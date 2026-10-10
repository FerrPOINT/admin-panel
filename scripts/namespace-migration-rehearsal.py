"""Read-only snapshots, isolated restore and additive namespace migration rehearsal.
Never modifies the source database/container. Removes only its own ephemeral container.
Run from the PDLC root: python admin-panel/scripts/namespace-migration-rehearsal.py
"""
import concurrent.futures, hashlib, json, subprocess, sys, time, uuid
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
SOURCE = "pdlc-common-postgres-1"
DOCKER_CONTEXT = None
DATABASES = {
    "adminpanel": "admin-panel/backend/migration/migrations/0011_namespaces.sql",
    "tasktracker": "task-tracker/backend/migration/src/namespace.sql",
    "wiki": "wiki/backend/migrations/202610080001_namespace.up.sql",
    "cicd": "CI-CD/backend/migrations/0090_namespaces_and_repository_identity.sql",
    "fleet_control": "fleet-control/backend/migration/src/execution_context.sql",
    "project_workflow": "project-workflow/project_workflow/infrastructure/db/migrations/versions/0007_resource_execution_contexts.py",
}

def run(*args, data=None, ok=True):
    if args and args[0]=='docker' and DOCKER_CONTEXT:
        args=('docker','--context',DOCKER_CONTEXT,*args[1:])
    result = subprocess.run(args, input=data, capture_output=True, check=False)
    if ok and result.returncode:
        raise RuntimeError(f"Command failed ({args[0]}): {result.stderr.decode(errors='replace')[-4000:]}")
    return result

def main():
    global DOCKER_CONTEXT
    DOCKER_CONTEXT=subprocess.check_output(['docker','context','show'],text=True).strip()
    out = ROOT / '.local/reports' / ('namespace-rehearsal-' + time.strftime('%Y%m%d-%H%M%S'))
    out.mkdir(parents=True, exist_ok=False)
    report = {'source': SOURCE, 'docker_context': DOCKER_CONTEXT, 'snapshots': {}, 'checks': [], 'state': 'in_progress'}
    legacy_repo,legacy_pr,unresolved_pr,legacy_dossier=[str(uuid.uuid4()) for _ in range(4)]
    qa_project = 'sdlc-qa-namespace-' + uuid.uuid4().hex[:12]
    container = qa_project + '-postgres'
    compose_file=out/'compose.json'
    compose=['docker','compose','-p',qa_project,'-f',str(compose_file)]
    compose_file.write_text(json.dumps({'name':qa_project,'services':{'postgres':{
        'image':'postgres:17','container_name':container,'network_mode':'none',
        'tmpfs':['/var/lib/postgresql/data:rw'],
        'environment':{'POSTGRES_HOST_AUTH_METHOD':'trust'},
        'labels':{'sdlc.owner':'shared-namespace-20261008','sdlc.purpose':'namespace-migration-rehearsal'},
    }}}),encoding='utf-8')
    own_id = None
    def sql(db, statement, expect_ok=True):
        return run('docker','exec','-i',container,'psql','-U','postgres','-d',db,'-v','ON_ERROR_STOP=1','-At',data=statement.encode(),ok=expect_ok)
    try:
        for db in DATABASES:
            dump = run('docker','exec',SOURCE,'pg_dump','-U','sdlc_admin','-Fc','--no-owner','--no-acl',db).stdout
            (out / (db+'.dump')).write_bytes(dump)
            report['snapshots'][db]={'sha256':hashlib.sha256(dump).hexdigest(),'bytes':len(dump)}
        if run('docker','ps','-aq','--filter',f'label=com.docker.compose.project={qa_project}').stdout.strip():raise RuntimeError('QA project already exists')
        run(*compose,'up','-d','--no-build','--pull','never','postgres')
        own_id=run('docker','inspect','--format','{{.Id}}',container).stdout.decode().strip()
        for _ in range(60):
            if run('docker','exec',container,'pg_isready','-U','postgres',ok=False).returncode==0:break
            time.sleep(0.5)
        else:raise RuntimeError('Ephemeral PostgreSQL did not start')
        for db,path in DATABASES.items():
            run('docker','exec',container,'createdb','-U','postgres',db)
            run('docker','exec','-i',container,'pg_restore','-U','postgres','--no-owner','--no-acl','-d',db,data=(out/(db+'.dump')).read_bytes())
            table = {'adminpanel':'audit_events','tasktracker':'issues','wiki':'documents','cicd':'projects','fleet_control':'agent_sessions','project_workflow':'project_workflow.tasks'}[db]
            restored=sql(db,f'SELECT count(*) FROM {table};').stdout.decode().strip()
            original=run('docker','exec',SOURCE,'psql','-U','sdlc_admin','-d',db,'-Atc',f'SELECT count(*) FROM {table};').stdout.decode().strip()
            if restored!=original:raise RuntimeError(f'{db}: restored inventory drift; repeat snapshot during quiet window')
            if db=='tasktracker':
                foundation=(ROOT/'task-tracker/backend/migration/src/m20261001_0000034_sdlc_clarification.rs').read_text()
                sql(db,'BEGIN;'+foundation.split('pub const UP_SQL: &str = r#"',1)[1].split('"#;',1)[0]+'COMMIT;')
            if db=='cicd':
                sql(db,'BEGIN;'+(ROOT/'CI-CD/backend/migrations/0039_runner_completion_readback.sql').read_text()+'COMMIT;')
                # Explicit fixtures enrich the restored copy; no synthetic data reaches the source.
                sql(db,f"INSERT INTO repositories(id,name) VALUES('{legacy_repo}','qa-legacy-flat'); INSERT INTO pull_requests(id,repository_name,number,title,source_branch,target_branch,status) VALUES('{legacy_pr}','qa-legacy-flat',31,'Historical PR','feature','main','closed'),('{unresolved_pr}','qa-unresolved-history',8,'Unresolved historical PR','feature','main','closed');")
            if db=='wiki':
                sql(db,f"INSERT INTO task_dossiers(id,space_id,task_key,title_snapshot,external_url) SELECT '{legacy_dossier}',id,'QA-LEGACY-7','Historical task','https://example.test/historical/task' FROM spaces ORDER BY id LIMIT 1;")
            migration=(ROOT/path).read_bytes()
            if db=='project_workflow':
                assert sql(db,'SELECT version_num FROM project_workflow.alembic_version;').stdout.decode().strip()=='0006_versioned_mode_catalog'
                python=ROOT/'.local/workflow-venv/Scripts/python.exe'
                emitter="import importlib.util,io,sys; from alembic.migration import MigrationContext; from alembic.operations import Operations; spec=importlib.util.spec_from_file_location('namespace_migration',sys.argv[1]); module=importlib.util.module_from_spec(spec); spec.loader.exec_module(module); output=io.StringIO(); context=MigrationContext.configure(dialect_name='postgresql',opts={'as_sql':True,'output_buffer':output});\nwith Operations.context(context): module.upgrade()\nprint(output.getvalue())"
                ddl=run(str(python) if python.exists() else sys.executable,'-c',emitter,str(ROOT/path)).stdout.decode()
                sql(db,'BEGIN; SET LOCAL search_path=project_workflow;\n'+ddl+'\nCOMMIT;')
            else:
                sql(db,'BEGIN;\n'+migration.decode()+'\nCOMMIT;')
            if db=='cicd':
                assert sql(db,f"SELECT id::text||'|'||repository_id::text||'|'||number FROM pull_requests WHERE id='{legacy_pr}';").stdout.decode().strip()==f'{legacy_pr}|{legacy_repo}|31'
                assert sql(db,f"SELECT repository_id IS NULL FROM pull_requests WHERE id='{unresolved_pr}';").stdout.decode().strip()=='t'
                assert sql(db,"SELECT count(*) FROM repository_catalog WHERE storage_name='qa-unresolved-history';").stdout.decode().strip()=='0'
                assert sql(db,f"SELECT alias FROM repository_aliases WHERE repository_id='{legacy_repo}';").stdout.decode().strip()=='qa-legacy-flat'
                assert sql(db,f"SELECT allocate_repository_pr_number('{legacy_repo}');").stdout.decode().strip()=='32'
                report['checks'].append({'scenario':'forge_legacy_pr_identity_numbers_flat_alias_and_unresolved_history','status':'passed'})
            if db=='wiki':
                assert sql(db,f"SELECT task_key||'|'||external_url||'|'||(task_id IS NULL)::text FROM task_dossiers WHERE id='{legacy_dossier}';").stdout.decode().strip()=='QA-LEGACY-7|https://example.test/historical/task|true'
                tracker=str(uuid.uuid4())
                for _ in range(2):
                    sql(db,f"INSERT INTO task_dossiers(id,space_id,task_key,tracker_instance_id,task_id) SELECT '{uuid.uuid4()}',space_id,task_key,'{tracker}','{uuid.uuid4()}' FROM task_dossiers WHERE id='{legacy_dossier}';")
                assert sql(db,"SELECT count(*) FROM task_dossiers WHERE task_key='QA-LEGACY-7';").stdout.decode().strip()=='3'
                report['checks'].append({'scenario':'wiki_legacy_dossier_and_same_display_key_distinct_task_refs','status':'passed'})
            report['checks'].append({'database':db,'restore_count':int(restored),'migration_sha256':hashlib.sha256(migration).hexdigest(),'status':'passed'})
        project=sql('tasktracker','SELECT id FROM projects ORDER BY id LIMIT 1;').stdout.decode().strip()
        if project:
            def allocate(_):return int(sql('tasktracker',f"SELECT allocate_project_issue_number('{project}');").stdout.decode().strip())
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:numbers=list(pool.map(allocate,range(32)))
            assert len(set(numbers))==32 and max(numbers)-min(numbers)==31, numbers
            report['checks'].append({'scenario':'concurrent_counter','allocations':32,'status':'passed'})
            registry,namespace,instance,operation=[str(uuid.uuid4()) for _ in range(4)]
            command={'schema_version':1,'namespace':{'registry_instance_id':registry,'namespace_id':namespace},'resource':{'kind':'tracker_project','instance_id':instance,'resource_id':project},'operation_id':operation,'generation':1,'state':'archived','create_spec':None}
            payload=json.dumps(command)
            sql('tasktracker',f"INSERT INTO tracker_namespace_bindings VALUES('{project}','{registry}','{namespace}',1,'archived','{payload}'::jsonb);")
            assert sql('tasktracker',f"SELECT allocate_project_issue_number('{project}');",False).returncode!=0
            assert sql('tasktracker',f"UPDATE projects SET name=name WHERE id='{project}';",False).returncode!=0
            assert sql('tasktracker',f"DELETE FROM projects WHERE id='{project}';",False).returncode!=0
            sql('tasktracker',f"UPDATE tracker_namespace_bindings SET state='active',generation=2,command=jsonb_set(jsonb_set(command,'{{state}}','\"active\"'),'{{generation}}','2') WHERE resource_id='{project}';")
            sql('tasktracker',f"UPDATE projects SET name=name WHERE id='{project}';")
            assert sql('tasktracker',f"DELETE FROM projects WHERE id='{project}';",False).returncode!=0
            report['checks'].append({'scenario':'tracker_archive_restore_legacy_paths','status':'passed'})
        # Canonical reservation is unique under concurrent attach, even with identical display names.
        registry,instance,resource=[str(uuid.uuid4()) for _ in range(3)]
        candidates=[]
        for index in range(2):
            ns,op=[str(uuid.uuid4()) for _ in range(2)]
            sql('adminpanel',f"INSERT INTO namespaces(id,registry_instance_id,slug,name,responsible_subject,state) VALUES('{ns}','{registry}','qa-{index}','Same display name','qa','provisioning'); INSERT INTO namespace_operations(id,namespace_id,actor_subject,command,state) VALUES('{op}','{ns}','qa','{{\"action\":\"attach\"}}','pending');")
            candidates.append((ns,op))
        def reserve(candidate):
            ns,op=candidate
            return sql('adminpanel',f"INSERT INTO namespace_bindings(namespace_id,kind,resource_instance_id,resource_id,operation_id,generation,desired_state) VALUES('{ns}','tracker_project','{instance}','{resource}','{op}',1,'active');",False).returncode
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool: outcomes=list(pool.map(reserve,candidates))
        assert outcomes.count(0)==1,outcomes
        report['checks'].append({'scenario':'concurrent_canonical_resource_reservation','status':'passed'})

        def bind(db,table,kind,resource,state='active'):
            registry,namespace,instance,op=[str(uuid.uuid4()) for _ in range(4)]
            command={'schema_version':1,'namespace':{'registry_instance_id':registry,'namespace_id':namespace},'resource':{'kind':kind,'instance_id':instance,'resource_id':resource},'operation_id':op,'generation':1,'state':state,'create_spec':None}
            sql(db,f"INSERT INTO {table} VALUES('{resource}','{registry}','{namespace}',1,'{state}','{json.dumps(command)}'::jsonb);")
        def transition(db,table,resource,state,generation):
            sql(db,f"UPDATE {table} SET state='{state}',generation={generation},command=jsonb_set(jsonb_set(command,'{{state}}','\"{state}\"'),'{{generation}}','{generation}') WHERE resource_id='{resource}';")
        space=sql('wiki','SELECT id FROM spaces ORDER BY id LIMIT 1;').stdout.decode().strip()
        document=sql('wiki',f"SELECT id FROM documents WHERE space_id='{space}' ORDER BY id LIMIT 1;").stdout.decode().strip() if space else ''
        if space and document:
            bind('wiki','wiki_namespace_bindings','wiki_space',space)
            revision=sql('wiki',f"SELECT id FROM document_revisions WHERE document_id='{document}' ORDER BY version LIMIT 1;").stdout.decode().strip()
            if revision:
                assert sql('wiki',f"UPDATE document_revisions SET content_markdown=content_markdown||'changed' WHERE id='{revision}';",False).returncode!=0
                assert sql('wiki',f"DELETE FROM document_revisions WHERE id='{revision}';",False).returncode!=0
                report['checks'].append({'scenario':'wiki_published_revision_is_immutable','status':'passed'})
            sql('wiki',f"UPDATE documents SET archived_at=now(),status='archived' WHERE id='{document}';")
            transition('wiki','wiki_namespace_bindings',space,'archived',2)
            assert sql('wiki',f"UPDATE documents SET title=title WHERE id='{document}';",False).returncode!=0
            assert sql('wiki',f"DELETE FROM spaces WHERE id='{space}';",False).returncode!=0
            transition('wiki','wiki_namespace_bindings',space,'active',3)
            assert sql('wiki',f"SELECT archived_at IS NOT NULL FROM documents WHERE id='{document}';").stdout.decode().strip()=='t'
            report['checks'].append({'scenario':'wiki_restore_preserves_document_archive_and_identity','status':'passed'})

        groups,repositories=[],[]
        for index in range(2):
            group,repo=[str(uuid.uuid4()) for _ in range(2)]
            sql('cicd',f"INSERT INTO git_groups(id,slug,name) VALUES('{group}','qa-group-{index}','Same group name');")
            bind('cicd','forge_namespace_bindings','git_group',group)
            storage=f'qa-storage-{index}'
            sql('cicd',f"INSERT INTO repositories(id,name) VALUES('{repo}','{storage}'); UPDATE repository_catalog SET group_id='{group}',slug='same-repository' WHERE id='{repo}'; INSERT INTO repository_aliases(alias,repository_id) VALUES('qa-group-{index}/same-repository','{repo}');")
            groups.append(group);repositories.append(repo)
        assert sql('cicd',"SELECT count(*) FROM repository_catalog WHERE slug='same-repository';").stdout.decode().strip()=='2'
        config,pipeline,stage,job=[str(uuid.uuid4()) for _ in range(4)]
        sql('cicd',f"INSERT INTO projects(id,name,repository_url,repository_id) VALUES('{config}','QA delivery','https://example.test/qa.git','{repositories[0]}'); INSERT INTO pipelines(id,project_id,git_ref,status) VALUES('{pipeline}','{config}','main','queued'); INSERT INTO stages(id,pipeline_id,name,position,status) VALUES('{stage}','{pipeline}','test',0,'queued'); INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES('{job}','{stage}','QA','alpine','true',0,'queued');")
        transition('cicd','forge_namespace_bindings',groups[0],'archived',2)
        assert sql('cicd',f"INSERT INTO pipelines(id,project_id,git_ref,status) VALUES('{uuid.uuid4()}','{config}','main','queued');",False).returncode!=0
        assert sql('cicd',f"UPDATE jobs SET status='running' WHERE id='{job}';",False).returncode!=0
        assert sql('cicd',f"SELECT allocate_repository_pr_number('{repositories[0]}');",False).returncode!=0
        assert sql('cicd',f"UPDATE projects SET repository_id='{repositories[1]}' WHERE id='{config}';",False).returncode!=0
        transition('cicd','forge_namespace_bindings',groups[0],'active',3)
        sql('cicd',f"UPDATE jobs SET status='running' WHERE id='{job}';")
        transition('cicd','forge_namespace_bindings',groups[0],'archived',4)
        sql('cicd',f"UPDATE jobs SET status='success',finished_at=now() WHERE id='{job}';")
        report['checks'].append({'scenario':'forge_duplicate_slugs_archive_start_denial_and_drain','status':'passed'})
        user=sql('fleet_control','SELECT id FROM users ORDER BY id LIMIT 1;').stdout.decode().strip()
        agent=sql('fleet_control','SELECT id FROM agents ORDER BY id LIMIT 1;').stdout.decode().strip()
        if user and agent:
            session,operation=[str(uuid.uuid4()) for _ in range(2)]
            context={'schema_version':2,'operation_id':operation,
                'namespace':{'registry_instance_id':str(uuid.uuid4()),'namespace_id':str(uuid.uuid4())},
                'task':{'tracker_instance_id':str(uuid.uuid4()),'task_id':str(uuid.uuid4())},'repositories':[]}
            projection={'context':context,'tracker_project_id':str(uuid.uuid4()),'binding_generation':1,
                'runtime_ready':False,'dispatch_allowed':False,'adapter_version':'namespace-context-v2/foundation-v1-disabled'}
            sql('fleet_control',f"INSERT INTO agent_sessions(id,agent_id,user_id,title,state) VALUES('{session}','{agent}','{user}','Namespace context QA','draft'); INSERT INTO session_execution_contexts(session_id,operation_id,context,verified_projection,created_by_user_id) VALUES('{session}','{operation}','{json.dumps(context)}'::jsonb,'{json.dumps(projection)}'::jsonb,'{user}');")
            message=str(uuid.uuid4())
            assert sql('fleet_control',f"INSERT INTO session_messages(id,session_id,author_type,author_user_id,body,message_kind) VALUES('{message}','{session}','user','{user}','start','user_prompt');",False).returncode!=0
            assert sql('fleet_control',f"INSERT INTO session_agent_runs(id,session_id,agent_id,run_role,state) VALUES('{uuid.uuid4()}','{session}','{agent}','primary','pending');",False).returncode!=0
            sql('fleet_control',f"INSERT INTO session_messages(id,session_id,author_type,body,message_kind) VALUES('{message}','{session}','system','history','system_event');")
            assert sql('fleet_control',f"INSERT INTO message_dispatch_outbox(message_id,agent_id) VALUES('{message}','{agent}');",False).returncode!=0
            report['checks'].append({'scenario':'fleet_context_v2_closes_prompt_run_and_dispatch','status':'passed'})
        # Even loss of a binding projection cannot turn a formerly managed resource
        # back into a legacy writable resource. The marker itself cannot be cleared.
        for db,resource_table,binding_table,resource_id in [
            ('tasktracker','projects','tracker_namespace_bindings',project),
            ('wiki','spaces','wiki_namespace_bindings',space),
            ('cicd','git_groups','forge_namespace_bindings',groups[0]),
        ]:
            if not resource_id:continue
            for patch in ({'unexpected_v1_field':True},{'generation':'3'},{'operation_id':'00000000-0000-0000-0000-000000000000'}):
                payload=json.dumps(patch)
                assert sql(db,f"UPDATE {binding_table} SET command=command||'{payload}'::jsonb WHERE resource_id='{resource_id}';",False).returncode!=0
            report['checks'].append({'scenario':f'{db}_invalid_projection_rejected','status':'passed'})
            assert sql(db,f"UPDATE {resource_table} SET namespace_managed=false WHERE id='{resource_id}';",False).returncode!=0
            sql(db,f"DELETE FROM {binding_table} WHERE resource_id='{resource_id}';")
            assert sql(db,f"UPDATE {resource_table} SET name=name WHERE id='{resource_id}';",False).returncode!=0
            report['checks'].append({'scenario':f'{db}_missing_projection_denies_write','status':'passed'})
        report['state']='passed'
    except Exception as error:
        report['state']='failed';report['error']=str(error)
        raise
    finally:
        if own_id is None:
            inspected=run('docker','inspect',container,ok=False)
            if inspected.returncode==0:
                actual_rows=json.loads(inspected.stdout)
                if len(actual_rows)==1 and actual_rows[0]['Config']['Labels'].get('com.docker.compose.project')==qa_project and actual_rows[0]['Config']['Labels'].get('sdlc.purpose')=='namespace-migration-rehearsal':own_id=actual_rows[0]['Id']
        if own_id:
            actual=run('docker','inspect','--format','{{.Id}}',container,ok=False)
            if actual.returncode==0 and actual.stdout.decode().strip()==own_id:
                run(*compose,'down')
            else:report['cleanup']='container identity changed; not removed'
        (out/'evidence.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(out/'evidence.json')

if __name__=='__main__':main()
