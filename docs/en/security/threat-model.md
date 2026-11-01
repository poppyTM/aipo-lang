# Threat model (STRIDE)

| Threat | Potential vector | Mandatory mitigation |
|--------|------------------|----------------------|
| **Spoofing** | Forgery of calls or payloads | Mutual authentication and strict signature validation. |
| **Tampering** | Malicious modification of goals or code | Integrity verification via cryptographic SHA-256 digest and protected-branch rules. |
| **Repudiation** | Actions executed without traceability | Immutable event log and evidence linked to goals. |
| **Information disclosure** | Accidental exposure of credentials or data | Automated pre-commit scanning and secret redaction in logs. |
| **Denial of service** | Excessive resource consumption or loops | Context budget limits (LPC), explicit timeouts and no unbounded recursion. |
| **Elevation of privilege** | Escape from sub-agent isolation | Connectors with strict execution policies and permission isolation. |
