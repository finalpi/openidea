param(
    [Parameter(Mandatory=$true)][string]$Project,
    [int]$Runs = 5,
    [string]$Output = (Join-Path $env:TEMP ('openidea-index-' + [guid]::NewGuid() + '.json'))
)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$projectRoot = (Resolve-Path -LiteralPath $Project).Path
$probe = Join-Path $repoRoot 'target/debug/examples/resource_probe.exe'
if (!(Test-Path -LiteralPath $probe)) { throw 'Build first: cargo build -p ide-index --example resource_probe' }
$cacheRoot = Join-Path $env:TEMP ('openidea-index-probe-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $cacheRoot | Out-Null
$samples = foreach ($run in 1..$Runs) {
    foreach ($mode in $(if ($run % 2) { @('symbols', 'files-only') } else { @('files-only', 'symbols') })) {
        $start = [System.Diagnostics.ProcessStartInfo]::new($probe)
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardInput = $true
        $start.ArgumentList.Add($projectRoot)
        $start.ArgumentList.Add((Join-Path $cacheRoot "$run-$mode"))
        if ($mode -eq 'files-only') { $start.ArgumentList.Add('--files-only') }
        $process = [System.Diagnostics.Process]::Start($start)
        try {
            $line = $process.StandardOutput.ReadLine()
            if (!$line) { throw 'Index probe exited without a result' }
            $index = $line | ConvertFrom-Json
            $process.Refresh()
            [pscustomobject]@{
                run = $run; mode = $mode
                files = $index.files; symbols = $index.symbols
                elapsedMs = $index.elapsed_ms
                cpuSeconds = $process.TotalProcessorTime.TotalSeconds
                workingSetBytes = $process.WorkingSet64
                privateCommitBytes = $process.PrivateMemorySize64
            }
        } finally {
            $process.StandardInput.WriteLine('done')
            $process.WaitForExit()
            $process.Dispose()
        }
    }
}
$report = [pscustomobject]@{
    scope = 'Isolated index process; debug build; cold index caches; OS file cache not cleared. Not whole-IDE measurements.'
    project = $projectRoot; cache = $cacheRoot; samples = @($samples)
}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Output -Encoding utf8
$samples | Format-Table
Write-Output "Report: $Output"
