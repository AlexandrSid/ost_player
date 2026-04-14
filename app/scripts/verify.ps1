param(
  [switch]$FixFmt
)

$ErrorActionPreference = "Stop"

function Run-Step([string]$Name, [scriptblock]$Cmd) {
  Write-Host ""
  Write-Host "==> $Name"
  & $Cmd
  if ($LASTEXITCODE -ne 0) {
    throw "Step failed: $Name (exit code $LASTEXITCODE)"
  }
}

Push-Location $PSScriptRoot\..
try {
  if ($FixFmt) {
    Run-Step "cargo fmt" { cargo fmt --all }
  } else {
    Run-Step "cargo fmt (check)" { cargo fmt --all -- --check }
  }

  Run-Step "cargo clippy" { cargo clippy --all-targets --all-features -- -D warnings }
  Run-Step "cargo test" { cargo test --all }

  Write-Host ""
  Write-Host "All checks passed."
} finally {
  Pop-Location
}

