# ERRORS.md — registre central des erreurs SOS-GUIDE

> **Fichier vivant.** Deux mécanismes l'alimentent et le font évoluer :
>
> 1. **Capture automatique (sans token)** — toute commande `just build|check|pi`
>    qui échoue dépose une entrée brute dans l'[Inbox](#-inbox--captures-brutes)
>    via `just _capture` (30 dernières lignes seulement, pour rester léger).
> 2. **Curation par agent** — l'agent
>    [`error-curator`](agents/error-curator.md) trie l'inbox,
>    déduplique, qualifie la cause racine, classe en *Actives* / *Résolues*,
>    et intègre les erreurs runtime du Pi (`just logs`). Lancement :
>    `just errors` (ponctuel) ou `/loop 30m` avec l'agent (surveillance continue).
>
> Format d'une entrée curée :
> `### ERR-NNN — titre court` · **Statut** · **Où** (crate/commande/Pi) ·
> **Cause** · **Correctif** · **Vu le**.

## 🔴 Actives

_Aucune erreur active._

## ✅ Résolues

### ERR-002 — `just install` ne redéployait pas le code à chaud

- **Statut** : Résolue
- **Où** : `audit-control/justfile` (recette `install`) + runtime Pi
- **Cause** : `systemctl enable --now` **ne redémarre pas** un service déjà
  actif. Après `install`, le binaire et le webroot étaient bien copiés, mais
  l'ancien process restait en mémoire — d'où `http://192.168.1.133/` en 404
  (vieux binaire sans webroot). Webroot `/var/www/sos-guide` également absent
  (jamais déployé avant l'ajout du push webroot au justfile).
- **Correctif** : `enable` puis `restart` explicite dans la recette `install`.
  Vérifié sur le Pi : nouveau process, `/`, `/install`, `/audit`, `/api/status`
  en HTTP 200, identité persistée dans Redb.
- **Vu le** : 2026-06-13

### ERR-001 — Capture automatique d'erreurs inopérante

- **Statut** : Résolue
- **Où** : `audit-control/justfile` (recettes `build` / `check` / `pi`)
- **Cause** : les recettes font `cd {{box}}` (dossier produit `sosguide/`, sans
  justfile) puis appellent `just _capture` ; `just` ne trouve alors aucun
  justfile (« no justfile found ») et la capture est perdue. Bug latent depuis
  l'origine (déjà présent avec l'ancienne disposition `sg-box`/`sg-claude`).
- **Correctif** : invoquer `just --justfile "{{justfile()}}" _capture …`
  (référence explicite au justfile). Vérifié : une commande échouée dépose
  désormais bien sa trace dans l'inbox.
- **Vu le** : 2026-06-13

## 📥 Inbox — captures brutes

<!-- Les recettes `just` ajoutent ici. L'agent error-curator vide cette section
     après avoir promu chaque capture en entrée curée ci-dessus. -->

_Inbox vide._

- `2026-06-24T14:01:57+02:00` — échec `cargo build --release (Pi)`
  ```
     Compiling sos-portal v0.1.0 (/home/admin/sosguide-v1/crates/portal)
     Compiling sos-guide v0.1.0 (/home/admin/sosguide-v1/apps/sos-guide)
  ```
