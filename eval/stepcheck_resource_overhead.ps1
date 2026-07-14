param(
  [string]$LatencyFile = "eval/aws-validate-latency.json",
  [string]$Out = "eval/stepcheck-resource-overhead.json",
  [string]$Bin = "stepcheck/target/release/stepcheck.exe",
  [int]$Limit = 0
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$LatencyPath = if ([System.IO.Path]::IsPathRooted($LatencyFile)) { $LatencyFile } else { Join-Path $Root $LatencyFile }
$OutPath = if ([System.IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path $Root $Out }
$BinPath = if ([System.IO.Path]::IsPathRooted($Bin)) { $Bin } else { Join-Path $Root $Bin }

if (!(Test-Path $LatencyPath)) { throw "missing latency file: $LatencyPath" }
if (!(Test-Path $BinPath)) { throw "missing StepCheck binary: $BinPath" }

$Latency = Get-Content $LatencyPath -Raw | ConvertFrom-Json
$Items = @($Latency.details)
if ($Limit -gt 0) { $Items = @($Items | Select-Object -First $Limit) }

function Percentile($Values, [double]$P) {
  $Sorted = @($Values | Sort-Object)
  if ($Sorted.Count -eq 0) { return $null }
  $Index = [Math]::Min($Sorted.Count - 1, [Math]::Max(0, [Math]::Ceiling($P * $Sorted.Count) - 1))
  return [Math]::Round([double]$Sorted[$Index], 1)
}

function Summary($Rows, [string]$Field) {
  $Values = @($Rows | ForEach-Object { $_.$Field } | Where-Object { $_ -ne $null })
  if ($Values.Count -eq 0) { return @{ n = 0 } }
  $Sum = 0.0
  foreach ($Value in $Values) { $Sum += [double]$Value }
  return @{
    n = $Values.Count
    mean = [Math]::Round($Sum / $Values.Count, 1)
    p50 = Percentile $Values 0.50
    p95 = Percentile $Values 0.95
    max = [Math]::Round([double](@($Values | Sort-Object)[$Values.Count - 1]), 1)
    min = [Math]::Round([double](@($Values | Sort-Object)[0]), 1)
  }
}

$Rows = @()
foreach ($Item in $Items) {
  $Psi = New-Object System.Diagnostics.ProcessStartInfo
  $Psi.FileName = $BinPath
  $Psi.WorkingDirectory = $Root
  $Psi.Arguments = 'check --json --infer "' + $Item.file + '"'
  $Psi.UseShellExecute = $false
  $Psi.RedirectStandardOutput = $true
  $Psi.RedirectStandardError = $true
  $Psi.CreateNoWindow = $true

  $Proc = New-Object System.Diagnostics.Process
  $Proc.StartInfo = $Psi
  $PeakWorkingSet = 0L
  $Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
  [void]$Proc.Start()
  try {
    $Proc.Refresh()
    if ($Proc.WorkingSet64 -gt $PeakWorkingSet) { $PeakWorkingSet = $Proc.WorkingSet64 }
  } catch {}
  while (-not $Proc.WaitForExit(1)) {
    try {
      $Live = Get-Process -Id $Proc.Id -ErrorAction Stop
      if ($Live.WorkingSet64 -gt $PeakWorkingSet) { $PeakWorkingSet = $Live.WorkingSet64 }
    } catch {}
  }
  try {
    $Live = Get-Process -Id $Proc.Id -ErrorAction Stop
    if ($Live.WorkingSet64 -gt $PeakWorkingSet) { $PeakWorkingSet = $Live.WorkingSet64 }
  } catch {}
  $Stdout = $Proc.StandardOutput.ReadToEnd()
  $Stderr = $Proc.StandardError.ReadToEnd()
  $Proc.WaitForExit()
  $Stopwatch.Stop()

  $BaselineWallMs = $null
  if ($Item.stepcheck -and $Item.stepcheck.ms -ne $null) { $BaselineWallMs = [double]$Item.stepcheck.ms }
  $CpuMs = [Math]::Round($Proc.TotalProcessorTime.TotalMilliseconds, 1)
  $CpuPctOfOneCore = $null
  if ($BaselineWallMs -and $BaselineWallMs -gt 0) {
    $CpuPctOfOneCore = [Math]::Round(100.0 * $CpuMs / $BaselineWallMs, 1)
  }

  $Rows += [pscustomobject]@{
    id = $Item.id
    file = $Item.file
    baseline_wall_ms = $BaselineWallMs
    instrumented_wall_ms = [Math]::Round($Stopwatch.Elapsed.TotalMilliseconds, 1)
    cpu_time_ms = $CpuMs
    cpu_pct_of_one_core = $CpuPctOfOneCore
    peak_working_set_mb = [Math]::Round($PeakWorkingSet / 1MB, 1)
    exit_code = $Proc.ExitCode
    stderr = if ($Stderr) { $Stderr.Trim().Substring(0, [Math]::Min(300, $Stderr.Trim().Length)) } else { $null }
  }
}

$TotalCpu = 0.0
$TotalWall = 0.0
foreach ($Row in $Rows) {
  $TotalCpu += [double]$Row.cpu_time_ms
  if ($Row.baseline_wall_ms -ne $null) { $TotalWall += [double]$Row.baseline_wall_ms }
}

$Report = [ordered]@{
  generated_at = (Get-Date).ToUniversalTime().ToString("o")
  generated_by = "eval/stepcheck_resource_overhead.ps1"
  latency_file = ($LatencyPath.Substring($Root.Length + 1) -replace "\\", "/")
  command = "stepcheck check --json --infer <workflow>"
  sample = @{
    workflows = $Rows.Count
    seed = $Latency.sampling.seed
    source = "same workflow sample as eval/aws-validate-latency.json"
  }
  summary = @{
    cpu_time_ms = Summary $Rows "cpu_time_ms"
    cpu_pct_of_one_core = Summary $Rows "cpu_pct_of_one_core"
    peak_working_set_mb = Summary $Rows "peak_working_set_mb"
    aggregate_cpu_time_ms = [Math]::Round($TotalCpu, 1)
    aggregate_baseline_wall_ms = [Math]::Round($TotalWall, 1)
    aggregate_cpu_pct_of_one_core = if ($TotalWall -gt 0) { [Math]::Round(100.0 * $TotalCpu / $TotalWall, 1) } else { $null }
  }
  details = $Rows
}

$Report | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 $OutPath
$Report.summary | ConvertTo-Json -Depth 6
if ($OutPath.StartsWith($Root)) {
  Write-Host ("-> " + ($OutPath.Substring($Root.Length + 1) -replace "\\", "/"))
} else {
  Write-Host ("-> " + $OutPath)
}