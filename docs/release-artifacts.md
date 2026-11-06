## Verify your download

Verification is optional but recommended. Extract the archive and open a terminal
in the folder containing the executable and verification files.

Signature verification checks that the binary was signed by the cpath maintainer
and has not changed since signing.

[Sigstore](https://docs.sigstore.dev/about/overview/) is an open-source service
for signing software and checking who signed it.
[Cosign](https://docs.sigstore.dev/cosign/verifying/verify/) is its command-line
tool; it uses the included `cpath.sigstore.json` bundle to verify cpath's signature.

[Install Cosign](https://docs.sigstore.dev/cosign/system_config/installation/),
then run:

Bash (Linux, macOS, or WSL):

```bash
cosign verify-blob \
  --bundle "cpath.sigstore.json" \
  --certificate-identity "raman.dzehtsiar@hettica.com" \
  --certificate-oidc-issuer "https://accounts.google.com" \
  "cpath"
```

PowerShell (Windows):

```powershell
cosign verify-blob `
  --bundle "cpath.sigstore.json" `
  --certificate-identity "raman.dzehtsiar@hettica.com" `
  --certificate-oidc-issuer "https://accounts.google.com" `
  "cpath.exe"
```

Success prints `Verified OK` and returns exit code `0`. If verification fails,
do not run the binary.
