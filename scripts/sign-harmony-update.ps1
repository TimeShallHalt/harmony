# PowerShell 7 on the Windows release machine. The private key stays in Windows CNG.
[CmdletBinding(DefaultParameterSetName='Sign')]
param(
    [Parameter(Mandatory, ParameterSetName='Initialize')][switch]$InitializeKey,
    [Parameter(Mandatory, ParameterSetName='Sign')][string]$Tag,
    [Parameter(Mandatory, ParameterSetName='Sign')][string]$Installer,
    [Parameter(Mandatory, ParameterSetName='Sign')][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$provider = [Security.Cryptography.CngProvider]::MicrosoftSoftwareKeyStorageProvider
$keyName = 'Harmony release updates v1'
$publicFile = Join-Path $PSScriptRoot '../packaging/keys/harmony-update.pub'
if (-not [Security.Cryptography.CngKey]::Exists($keyName, $provider)) {
    if (-not $InitializeKey) { throw 'The Harmony release-signing key is unavailable in this Windows account.' }
    if (Test-Path -LiteralPath $publicFile) { throw 'A publisher key is already pinned. Initialize on the original signing account or perform a deliberate key rotation.' }
    $parameters = [Security.Cryptography.CngKeyCreationParameters]::new()
    $parameters.Provider = $provider
    $parameters.KeyUsage = [Security.Cryptography.CngKeyUsages]::Signing
    $parameters.ExportPolicy = [Security.Cryptography.CngExportPolicies]::None
    $key = [Security.Cryptography.CngKey]::Create([Security.Cryptography.CngAlgorithm]::ECDsaP256, $keyName, $parameters)
} else {
    $key = [Security.Cryptography.CngKey]::Open($keyName, $provider)
}
$signer = [Security.Cryptography.ECDsaCng]::new($key)
try {
    $point = $signer.ExportParameters($false).Q
    $public = [Convert]::ToHexString([byte[]](@(4) + $point.X + $point.Y)).ToLowerInvariant()
    if (Test-Path -LiteralPath $publicFile) {
        if ((Get-Content -Raw -LiteralPath $publicFile).Trim() -cne $public) { throw 'This account does not have the pinned Harmony publisher key.' }
    } elseif ($InitializeKey) {
        [IO.File]::WriteAllText($publicFile, $public + [Environment]::NewLine)
    } else { throw 'The checkout is missing the pinned publisher key.' }
    if ($InitializeKey) { Write-Output "Harmony publisher public key: $public"; return }

    if ($Tag -cnotmatch '^harmony-v((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))-b([1-9][0-9]*)$') { throw 'Expected harmony-vMAJOR.MINOR.PATCH-bBUILD.' }
    $name = 'Harmony-' + $Matches[1] + '-Setup.exe'
    $expectedVersion = $Matches[1] + '.' + $Matches[2]
    $file = Get-Item -LiteralPath $Installer
    if ($file.Name -cne $name -or $file.Length -lt 64 -or $file.Length -gt 536870912) { throw 'Installer identity or length does not match the release.' }
    $metadata = $file.VersionInfo
    $embeddedVersion = '{0}.{1}.{2}.{3}' -f $metadata.FileMajorPart, $metadata.FileMinorPart, $metadata.FileBuildPart, $metadata.FilePrivatePart
    $installerDescriptions = @('Harmony Installer', 'Harmony Installer with Repair')
    if ($embeddedVersion -cne $expectedVersion -or $metadata.ProductName.Trim() -cne 'Harmony' -or
        $metadata.FileDescription.Trim() -cnotin $installerDescriptions) {
        throw 'Setup was not built for this Harmony release tag. Check the embedded product version and build number.'
    }
    $digest = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifest = [ordered]@{schema=1; repository='TimeShallHalt/harmony'; tag=$Tag; platform='windows-x64'; installer=$name; bytes=$file.Length; sha256=$digest}
    $bytes = [Text.Encoding]::UTF8.GetBytes(($manifest | ConvertTo-Json -Compress))
    $signature = $signer.SignData($bytes, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)
    if (-not $signer.VerifyData($bytes, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)) { throw 'Signature verification failed.' }
    $directory = [IO.Directory]::CreateDirectory([IO.Path]::GetFullPath($OutputDirectory))
    $out = Join-Path $directory.FullName 'harmony-update.json'
    if ((Test-Path -LiteralPath $out) -or (Test-Path -LiteralPath ($out + '.sig'))) { throw 'Choose a fresh output directory for this signature.' }
    [IO.File]::WriteAllBytes($out, $bytes)
    [IO.File]::WriteAllBytes($out + '.sig', $signature)
    Write-Output "Signed $Tag for $name ($digest)."
} finally { $signer.Dispose(); $key.Dispose() }
