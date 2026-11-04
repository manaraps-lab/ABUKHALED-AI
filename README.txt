ABUKHALED AI — first desktop branding integration patch

Contents:
- scripts/apply-branding.mjs: applies a guarded, idempotent branding patch to the checked-out upstream source.
- .github/workflows/upstream-windows-build.yml: updated build workflow that invokes the patch before dependency installation/build.

Important design safeguards:
- Requires OmniRoute Electron package version 3.8.53.
- Requires the reviewed upstream appId online.omniroute.desktop; aborts if it has drifted.
- Changes Electron productName and visible desktop-shell labels to ABUKHALED AI.
- Removes the inherited OmniRoute GitHub Releases publish target.
- Disables upstream auto-update checks/download/install until ABUKHALED AI has its own verified release feed.
- Deliberately preserves appId and OmniRoute data directory paths in this first phase to avoid splitting or losing existing credentials/database/preferences.

This patch does NOT yet rebrand the web dashboard, create Arabic/English UX, or produce a signed installer. Those are separate subsequent phases.
