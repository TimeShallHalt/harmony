# PowerShell 7 on the Windows release machine. The private key stays in Windows CNG.
[CmdletBinding(DefaultParameterSetName='Sign')]
param(
    [Parameter(Mandatory, ParameterSetName='Initialize')][switch]$InitializeKey,
    [Parameter(Mandatory, ParameterSetName='Sign')][string]$Tag,
    [Parameter(Mandatory, ParameterSetName='Sign')][Alias('AppImage')][string]$Installer,
    [Parameter(ParameterSetName='Sign')][ValidateSet('windows-x64','linux-x64')][string]$Platform = 'windows-x64',
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
    $version = $Matches[1]
    $build = $Matches[2]
    $name = if ($Platform -eq 'windows-x64') { "Harmony-$version-Setup.exe" } else { "Harmony-$version-x86_64.AppImage" }
    $file = Get-Item -LiteralPath $Installer
    if ($file.Name -cne $name -or $file.Length -lt 64 -or $file.Length -gt 536870912) { throw 'Package identity or length does not match the release.' }
    if ($Platform -eq 'windows-x64') {
        $metadata = $file.VersionInfo
        $embeddedVersion = '{0}.{1}.{2}.{3}' -f $metadata.FileMajorPart, $metadata.FileMinorPart, $metadata.FileBuildPart, $metadata.FilePrivatePart
        $installerDescriptions = @('Harmony Installer', 'Harmony Installer with Repair')
        if ($embeddedVersion -cne "$version.$build" -or $metadata.ProductName.Trim() -cne 'Harmony' -or $metadata.FileDescription.Trim() -cnotin $installerDescriptions) {
            throw 'Setup was not built for this Harmony release tag. Check the embedded product version and build number.'
        }
    } else {
        $header = [byte[]]::new(64)
        $image = [IO.File]::OpenRead($file.FullName)
        try { $image.ReadExactly($header) } finally { $image.Dispose() }
        if ([Convert]::ToHexString($header[0..5]) -cne '7F454C460201' -or
            [Convert]::ToHexString($header[8..10]) -cne '414902' -or
            [BitConverter]::ToUInt16($header,18) -ne 62) {
            throw 'Expected an AMD64 type 2 AppImage.'
        }
    }
    $digest = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifest = [ordered]@{schema=1; repository='TimeShallHalt/harmony'; tag=$Tag; platform=$Platform; installer=$name; bytes=$file.Length; sha256=$digest}
    $bytes = [Text.Encoding]::UTF8.GetBytes(($manifest | ConvertTo-Json -Compress))
    $signature = $signer.SignData($bytes, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)
    if (-not $signer.VerifyData($bytes, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)) { throw 'Signature verification failed.' }
    $directory = [IO.Directory]::CreateDirectory([IO.Path]::GetFullPath($OutputDirectory))
    $manifestName = if ($Platform -eq 'windows-x64') { 'harmony-update.json' } else { 'harmony-update-linux-x64.json' }
    $out = Join-Path $directory.FullName $manifestName
    if ((Test-Path -LiteralPath $out) -or (Test-Path -LiteralPath ($out + '.sig'))) { throw 'Choose a fresh output directory for this signature.' }
    [IO.File]::WriteAllBytes($out, $bytes)
    [IO.File]::WriteAllBytes($out + '.sig', $signature)
    Write-Output "Signed $Tag for $name ($digest)."
} finally { $signer.Dispose(); $key.Dispose() }
