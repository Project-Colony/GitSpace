# .NET interop

Ce document décrit la communication JSON entre GitSpace (Rust) et le helper .NET.

## Commande `ping`

Le helper .NET accepte un JSON sur `stdin` et renvoie un JSON sur `stdout`.

### Requête

```json
{"command":"ping"}
```

### Réponse

```json
{"status":"ok","message":"pong"}
```

## Exemple côté Rust

```rust
use crate::dotnet::{DotnetClient, DotnetRequest};

let request = DotnetRequest {
    command: "ping".to_string(),
};

let response = DotnetClient::new("dotnet")
    .with_args([
        "run",
        "--project",
        "dotnet/GitSpace.Helper/GitSpace.Helper.csproj",
    ])
    .send_request(&request)?;

assert_eq!(response.status, "ok");
assert_eq!(response.message, "pong");
```

## Exemple côté shell

```bash
echo '{"command":"ping"}' | dotnet run --project dotnet/GitSpace.Helper/GitSpace.Helper.csproj
```
