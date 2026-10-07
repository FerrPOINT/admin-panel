"""Check AI storage, migrations and messaging using disposable owned Compose resources."""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT.parent / 'services-base'
DATABASES = ['admin_migration_test', 'sdlc2_ai_registry_test', 'admin_panel_audit_test',
             'admin_messaging_test', 'admin_messaging_transport_test']


def prepare(directory: Path, project: str) -> Path:
    source = BASE / 'scripts/test_messaging_transport.py'
    spec = importlib.util.spec_from_file_location('messaging_qa', source)
    sdk = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(sdk)
    path = sdk.prepare(directory, project)
    config = json.loads(path.read_text())
    databases = DATABASES
    bind = lambda source, target: {'type': 'bind', 'source': str(source.resolve()), 'target': target,
                                   'read_only': True, 'bind': {'create_host_path': False}}
    check = config['services']['check']
    check['image'] = 'rust:1.88.0-bookworm'
    check['working_dir'] = '/workspace/admin-panel/backend'
    check['volumes'][0:1] = [bind(ROOT, '/workspace/admin-panel'), bind(BASE, '/workspace/services-base')]
    urls = '\n'.join(f'export {name}="postgres://messaging:$password@postgres:5432/{database}"' for name, database in [
        ('ADMIN_MIGRATION_TEST_DATABASE_URL', databases[0]), ('AI_REGISTRY_TEST_DATABASE_URL', databases[1]),
        ('ADMIN_PANEL_AUDIT_TEST_DATABASE_URL', databases[2]), ('MESSAGING_PRODUCT_TEST_DATABASE_URL', databases[3])])
    command = '\n'.join([
        'set -eu', 'password=$(cat /secrets/postgres)', urls,
        'cargo test --locked -p admin-panel-migration historical_ledgers_new_install_and_sql_changes -- --ignored --exact',
        'cargo test --locked -p admin-panel-infra --test ai_profiles -- --ignored --exact publication_cas_audit_immutability_replay_and_restart',
        'cargo test --locked -p admin-panel-infra --test audit_pagination',
        'cargo test --locked -p admin-panel-infra --test registry_patch',
        'cargo test --locked -p admin-panel-infra --test messaging feed_atomicity_pagination_and_retention -- --ignored --exact',
        'cargo test --locked -p admin-panel-infra --test messaging product_capacity_sample -- --ignored --exact',
        'export MESSAGING_PRODUCT_TEST_DATABASE_URL="postgres://messaging:$password@postgres:5432/admin_messaging_transport_test"',
        'cargo test --locked -p admin-panel-infra --test messaging standalone_product_feed_transport -- --ignored --exact',
        'export ADMINP_DATABASE_URL="$MESSAGING_PRODUCT_TEST_DATABASE_URL"',
        'export ADMINP_AUTH_JWKS_URI="http://127.0.0.1:18771/oidc/jwks"',
        'export ADMINP_AUTH_CENTRAL_API_URL="http://127.0.0.1:18771"',
        'export ADMINP_AUTH__CENTRAL_JWKS_URI="http://127.0.0.1:18771/oidc/jwks"',
        'export ADMINP_AUTH__CENTRAL_ISSUER="http://127.0.0.1:18771"',
        'export ADMINP_AUTH__CENTRAL_LOGIN_URL="http://127.0.0.1:18771/auth/login"',
        'cargo test --locked -p admin-panel-api --test messaging product_feed_api -- --ignored --exact',
    ])
    # SQLx unit test module name must be included with --exact.
    command = command.replace('historical_ledgers_new_install_and_sql_changes -- --ignored --exact',
                              'tests::historical_ledgers_new_install_and_sql_changes -- --ignored --exact')
    check['command'] = ['bash', '-c', command.replace('$', '$$')]
    path.write_text(json.dumps(config, indent=2), encoding='utf-8')
    sdk.restrict(path)
    return path


def main():
    argparse.ArgumentParser(description=__doc__).parse_args()
    project = 'sdlc-qa-admin-foundation-' + uuid.uuid4().hex[:12]
    directory = ROOT / '.local' / 'foundation-qa' / project
    path = prepare(directory, project)
    compose = ['docker', 'compose', '-p', project, '-f', str(path)]
    try:
        subprocess.run(compose + ['up', '-d', '--wait', '--wait-timeout', '180', 'broker', 'postgres'], check=True, timeout=240)
        # The initializer runs as postgres, which cannot read a private Windows bind.
        # Use stdin only after the normal cluster/host authentication setup has finished.
        sql = '\n'.join(f'CREATE DATABASE {name};' for name in DATABASES)
        subprocess.run(compose + ['exec', '-T', 'postgres', 'psql', '-U', 'messaging', '-d', 'messaging', '-v', 'ON_ERROR_STOP=1'],
                       input=sql.encode('ascii'), check=True, capture_output=True, timeout=120)
        with (directory / 'checks.log').open('w', encoding='utf-8') as log:
            result = subprocess.run(compose + ['run', '--rm', '-T', '--no-deps', 'check'], stdout=log, stderr=subprocess.STDOUT, timeout=2400)
        if result.returncode:
            raise RuntimeError('Foundation integration failed; inspect the private checks.log')
        print('Migration ledger compatibility, AI publication/CAS, audit and messaging checks passed.')
    finally:
        cleanup = subprocess.run(compose + ['down', '--remove-orphans'], capture_output=True, timeout=120)
        if cleanup.returncode:
            raise RuntimeError('Owned Compose cleanup failed; retain manifest for recovery')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
