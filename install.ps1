# formality (fml) installer for Windows
# https://github.com/arvinduh/formality
#
# Thin compatibility shim. The real installer is generated and published by
# cargo-dist as a release asset (issue #134). This file stays at
# https://raw.githubusercontent.com/arvinduh/formality/main/install.ps1 only so
# the one-liner already printed by older `fml` binaries and copied into
# third-party docs keeps working. New docs point straight at the release asset:
#
#   powershell -c "irm https://github.com/arvinduh/formality/releases/latest/download/fml-installer.ps1 | iex"

$ErrorActionPreference = 'Stop'

$distInstallerUrl = 'https://github.com/arvinduh/formality/releases/latest/download/fml-installer.ps1'

Invoke-RestMethod -Uri $distInstallerUrl -UseBasicParsing | Invoke-Expression
