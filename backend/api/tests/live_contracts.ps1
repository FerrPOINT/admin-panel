param(
    [string]$Api = 'http://127.0.0.1:18771',
    [string]$Auth = 'http://127.0.0.1:18701'
)
$ErrorActionPreference = 'Stop'
if ($Api -ne 'http://127.0.0.1:18771' -or $Auth -ne 'http://127.0.0.1:18701') {
    throw 'This destructive fixture test is restricted to the isolated QA ports.'
}
$script:Count = 0
function Request($Method, $Url, $Expected, $Body = $null, $Token = '', $Extra = @{}) {
    $headers = @{} + $Extra
    if ($Token) { $headers.Authorization = "Bearer $Token" }
    $args = @{ Method = $Method; Uri = $Url; Headers = $headers; SkipHttpErrorCheck = $true; SkipHeaderValidation = $true }
    if ($null -ne $Body) {
        $args.Body = ConvertTo-Json $Body -Depth 12 -Compress
        $args.ContentType = 'application/json'
    }
    $response = Invoke-WebRequest @args
    if ($response.StatusCode -ne $Expected) {
        $detail = ($response.Content | ConvertFrom-Json -ErrorAction SilentlyContinue).error.message
        throw "$Method $Url expected $Expected, received $($response.StatusCode): $detail"
    }
    $script:Count++
    return $response
}
function Json($Response) { return $Response.Content | ConvertFrom-Json }
function Assert($Condition, $Message) { if (!$Condition) { throw $Message } }
$prefix = 'http-qa-' + [guid]::NewGuid().ToString('N')
$password = 'invalid-outside-isolated-qa-password'
$email = "manager-$prefix@example.invalid"
$null = Request POST "$Auth/auth/register" 201 @{ email = $email; username = $prefix; password = $password }
$token = (Json (Request POST "$Auth/auth/login" 200 @{ email = $email; password = $password })).access_token
$null = Request GET "$Api/api/v1/users" 401
$null = Request GET "$Api/api/v1/auth/me" 200 $null $token
for ($i = 0; $i -lt 3; $i++) {
    $user = Json (Request POST "$Api/api/v1/users" 202 @{ email = "$prefix-$i@example.invalid"; display_name = "QA $i" } $token)
    Assert ($user.status -eq 'pending' -and $user.setup_delivery_status -eq 'failed') 'Partial create must retain pending account'
}
$page = Request GET "$Api/api/v1/users?q=$prefix&status=pending&limit=2&offset=2" 200 $null $token @{ Origin = 'http://localhost:18772' }
Assert ($page.Headers['X-Total-Count'][0] -eq '3') 'Proxy must forward total, not page size'
Assert (@(Json $page).Count -eq 1) 'Second page must contain one user'
Assert (($page.Headers['Access-Control-Expose-Headers'] -join ',') -match 'x-total-count') 'CORS must expose total'
$legacy = Request GET "$Api/api/v1/users?q=$prefix&offset=0" 200 $null $token
Assert (@(Json $legacy).Count -eq 4) 'Legacy array request must include manager and pending users'
$null = Request GET "$Api/api/v1/users?status=bogus" 400 $null $token
$clamped = Request GET "$Api/api/v1/users?q=$prefix&limit=0&offset=-5" 200 $null $token
Assert (@(Json $clamped).Count -eq 1) 'Proxy bounds must clamp negative offset and zero limit'
$pat = Json (Request POST "$Api/api/v1/tokens" 201 @{ label = 'QA read only'; scopes = @('admin-panel:read'); expires_in_days = 1 } $token)
$null = Request GET "$Api/api/v1/services" 200 $null $pat.secret
$key = $prefix
$declaration = @{ declaration_version = 1; integration_base_url = 'https://api.example.invalid'; public_ui_url = 'https://example.invalid'; service_contract_version = '1.0'; capabilities = @('health.read', 'ui.render') }
$create = @{ service_key = $key; display_name = 'QA integration'; owner_team = 'QA'; declaration = $declaration }
$null = Request POST "$Api/api/v1/services" 403 $create $pat.secret
$entry = Json (Request POST "$Api/api/v1/services" 201 $create $token)
$detail = Json (Request GET "$Api/api/v1/services/$key" 200 $null $token)
$approved = Json (Request POST "$Api/api/v1/services/$key/approve" 200 @{ declaration_id = $detail.declarations[0].id } $token @{ 'If-Match' = [string]$entry.service.version })
$runtime = Request GET "$Api/api/v1/runtime/services" 200
$null = Request GET "$Api/api/v1/runtime/services" 304 $null '' @{ 'If-None-Match' = $runtime.Headers.ETag[0] }
$declaration.declaration_version = 2
$declaration.integration_base_url = 'https://api-v2.example.invalid'
$replacement = Json (Request PATCH "$Api/api/v1/services/$key" 200 @{ declaration = $declaration } $token @{ 'If-Match' = [string]$approved.service.version })
Assert ($replacement.service.status -eq 'active') 'Pending replacement must preserve active status'
$detail = Json (Request GET "$Api/api/v1/services/$key" 200 $null $token)
$pending = $detail.declarations | Where-Object approval_status -eq 'pending'
$body = @{ declaration_id = $pending.id }
$null = Request POST "$Api/api/v1/services/$key/reject" 403 $body $pat.secret @{ 'If-Match' = [string]$detail.service.version }
$null = Request POST "$Api/api/v1/services/unknown-$key/reject" 404 $body $token @{ 'If-Match' = '1' }
$null = Request POST "$Api/api/v1/services/$key/reject" 412 $body $token
$null = Request POST "$Api/api/v1/services/$key/reject" 412 $body $token @{ 'If-Match' = '1' }
$null = Request POST "$Api/api/v1/services/$key/reject" 409 @{ declaration_id = [guid]::NewGuid().ToString() } $token @{ 'If-Match' = [string]$detail.service.version }
$rejection = Request POST "$Api/api/v1/services/$key/reject" 200 $body $token @{ 'If-Match' = [string]$detail.service.version }
$result = Json $rejection
Assert ($result.service.status -eq 'active') 'Rejection must preserve service status'
Assert ($result.service.active_declaration_id -eq $approved.service.active_declaration_id) 'Rejection must preserve active declaration'
Assert ($result.declaration.approval_status -eq 'rejected' -and !$result.declaration.approved_by_subject -and !$result.declaration.approved_at) 'Rejection must not populate approval fields'
Assert ($result.service.version -eq $detail.service.version + 1) 'Rejection must increment version once'
$null = Request POST "$Api/api/v1/services/$key/reject" 409 $body $token @{ 'If-Match' = [string]$result.service.version }
$unchanged = Request GET "$Api/api/v1/runtime/services" 200
Assert ($runtime.Content -eq $unchanged.Content) 'Replacement and rejection must preserve runtime projection'
$events = Json (Request GET "$Api/api/v1/audit-events?action=service.rejected&entity_type=service" 200 $null $token)
Assert (@($events.events | Where-Object entity_id -eq $result.service.id).Count -eq 1) 'Rejection must record exactly one decision event'
$ready = Request GET "$Api/health/ready" 200
Assert (!$ready.Content) 'Readiness must be empty 200'
$null = Request GET "$Api/api/v1/runtime/branding" 404
Write-Output "Isolated HTTP contracts passed: $script:Count requests; no tokens or fixture secrets printed."
