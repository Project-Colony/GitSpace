# Contrats .NET (IPC)

Ce document décrit les contrats d’échange entre le binaire Rust et le helper .NET via un canal IPC local.
Note d’alignement : validé le 2025-03-09.

## Endpoints / commands supportés

Tous les appels sont décrits comme un message JSON avec une enveloppe commune :

```json
{
  "id": "req-0001",
  "command": "dialog.open",
  "payload": {}
}
```

### `dialog.open`
Ouvre un dialogue natif (fichier, dossier, enregistrement).

**Payload (entrée)**
- `kind`: `open_file` | `open_folder` | `save_file`
- `title`: titre de la fenêtre
- `filters`: liste des filtres (`label`, `extensions`)
- `options`: options booléennes (`multi_select`, `show_hidden`)

**Réponse (sortie)**
- `selected_paths`: tableau de chemins
- `cancelled`: booléen

### `credential.request`
Demande un credential au provider système.

**Payload (entrée)**
- `service`: nom du service (ex: `github.com`)
- `account`: identifiant utilisateur (facultatif)
- `secret`: secret (token/mot de passe) requis pour `store`
- `action`: `get` | `store` | `erase`

**Réponse (sortie)**
- `username`: identifiant utilisateur
- `secret`: secret (mot de passe, token)
- `status`: `ok` | `not_found` | `denied`

**Erreurs Windows**

Sur Windows, les statuts sont dérivés des erreurs natives retournées par
`CredRead`, `CredWrite` et `CredDelete` :

| Code Win32 | Constante | Statut |
| --- | --- | --- |
| 1168 | `ERROR_NOT_FOUND` | `not_found` |
| 5 | `ERROR_ACCESS_DENIED` | `denied` |
| autre | (par défaut) | `denied` |

**Erreurs macOS**

Sur macOS, les statuts sont dérivés des erreurs natives retournées par
`SecKeychainFindGenericPassword`, `SecKeychainAddGenericPassword` et
`SecKeychainItemDelete` :

| OSStatus | Constante | Statut |
| --- | --- | --- |
| -25300 | `errSecItemNotFound` | `not_found` |
| -25293 | `errSecAuthFailed` | `denied` |
| autre | (par défaut) | `denied` |

### `library.call`
Appelle une bibliothèque .NET spécifique (interop ciblée).

**Payload (entrée)**
- `name`: `system.info`
- `payload`: objet JSON libre (contrat propre à la librairie)

**Réponse (sortie)**
- `payload`:
  - `os`: description de l’OS
  - `version`: version de l’OS
- `status`: `ok` | `error`
- `error`: erreur structurée (si `status` = `error`)

### `ping`
Vérifie la disponibilité du helper .NET.

**Payload (entrée)**
- `{}`

**Réponse (sortie)**
- `version`: version du helper

## Format d’entrée/sortie

- **Encodage**: JSON UTF-8.
- **Enveloppe commune**: `{ id, command, payload }` côté requête, `{ id, status, payload, error }` côté réponse.
- **Exemples JSON**: voir le dossier [`/schemas`](../schemas).

Exemple de réponse :

```json
{
  "id": "req-0001",
  "status": "ok",
  "payload": {
    "selected_paths": ["/Users/alex/Documents/report.pdf"],
    "cancelled": false
  }
}
```

## Codes d’erreur

Les erreurs remontent toujours un objet structuré :

```json
{
  "category": "InvalidRequest",
  "message": "Missing payload.kind",
  "details": {
    "field": "kind"
  }
}
```

| Catégorie | Quand ? | Message attendu |
| --- | --- | --- |
| `InvalidRequest` | Paramètres manquants ou invalides. | "Missing payload.kind" |
| `Internal` | Erreur interne .NET. | "Unhandled exception" |

Les messages doivent rester courts et exploitables côté UI (Rust). Les détails supplémentaires peuvent être placés dans `details`.
