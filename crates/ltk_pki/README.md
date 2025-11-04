Mod TOC signature validation support code.

# Limitations and Constraints
- Only RSA 2048-8192 pkcs1 signatures supported (artificial limitation for performance reasons)
- Certificates need to be X509 v3 (rustls webpki limitation)
- CRL webpki does not support:
  - CRL versions other than version 2.
  - CRLs missing the next update field.
  - CRLs missing certificate revocation list extensions.
  - Delta CRLs.
  - CRLs larger than (2^32)-1 bytes in size.

# Enforced policies
- Intermediate certificates MUST either:
  - Marked as `CA` in `BasicConstraints` https://datatracker.ietf.org/doc/html/rfc5280#section-4.2.1.9
  - Have either `CRLSign` or `KeyCertSign` bit in `KeyUsage` set https://datatracker.ietf.org/doc/html/rfc5280#section-4.2.1.3
- End certificates (used to sign actual mods):
  - MUST not be marked as intermediate certificates
  - MAY have `DigitalSignature` bit set  https://datatracker.ietf.org/doc/html/rfc5280#section-4.2.1.3
  - MAY have `codeSigning` EKU set https://oidref.com/1.3.6.1.5.5.7.3.3
- Each intermediate certificate MUST have a valid CRL
- Current date MUST be before next update time for any CRL
- Not before should be set at least 1 month prior to current date.
