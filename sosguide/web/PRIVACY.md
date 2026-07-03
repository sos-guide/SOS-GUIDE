# Politique de confidentialité — SOS-GUIDE

**Conformité :** Loi fédérale suisse sur la protection des données (nLPD, RS 235.1)  
**Conformité :** Règlement général sur la protection des données (RGPD, UE 2016/679)  
**Applicable à :** Toute installation du système SOS-GUIDE sur un Raspberry Pi  
**Dernière mise à jour :** Juin 2026

---

## 1. Responsable du traitement

Le responsable du traitement est l'entité qui déploie ce système sur le terrain
(mairie, hôpital, école, protection civile, Croix-Rouge, etc.).

Ludovic MARTIN / SOS-GUIDE agit en qualité de **sous-traitant technique**
(fournisseur du logiciel open source) conformément à l'art. 5 lit. j nLPD.

---

## 2. Données collectées — inventaire exhaustif

### 2.1 Données collectées par le portail captif (utilisateurs WiFi)

| Catégorie | Donnée | Base légale nLPD | Durée de conservation |
|-----------|--------|-------------------|----------------------|
| Réseau | Adresse IP locale DHCP (10.0.0.x) | Intérêt légitime (art. 31 nLPD) | **0 seconde** — tmpfs, jamais persistée |
| Réseau | Adresse MAC WiFi | Intérêt légitime (DHCP) | **0 seconde** — `dhcp-leasefile=/dev/null`, jamais écrite sur disque |
| Navigation | Requêtes HTTP | Aucune — serveur sans journal d'accès | **Néant** — le serveur web Axum ne journalise aucune requête |
| Contenu | Messages LoRa envoyés | Consentement explicite (art. 6 nLPD) | Tmpfs uniquement — supprimés au redémarrage par tmpfiles.d |

**Aucun cookie, aucun tracker, aucune donnée personnelle persistée sur disque.**

### 2.2 Données de configuration (administrateur local uniquement)

| Donnée | Finalité | Base légale | Conservation |
|--------|----------|-------------|--------------|
| Nom du lieu | Identification du nœud dans le réseau mesh | Nécessité contractuelle | Jusqu'à réinitialisation |
| Numéros d'urgence locaux | Affichage sur portail | Nécessité contractuelle | Jusqu'à modification admin |
| Coordonnées GPS (optionnel) | Affichage de la carte locale | Consentement | Jusqu'à modification admin |
| Adresse du lieu | Affichage sur portail | Nécessité contractuelle | Jusqu'à modification admin |

### 2.3 Données de messages LoRa (si module activé)

Les messages texte envoyés via le réseau LoRa sont :
- **Signés** (Ed25519) et protégés contre le rejeu — intégrité et authenticité
- **En mémoire volatile uniquement** pour l'affichage du portail (jamais sur disque)
- **Perdus au redémarrage** (logs et boîte de réception en tmpfs, conformité nLPD)
- **Pseudonymisés** : identifiant de nœud (8 hex) sans lien à une personne physique

---

## 3. Données non collectées (déclaration explicite)

SOS-GUIDE **ne collecte pas** et **ne peut pas collecter** :
- ✗ Contenu des navigations (sites visités par les clients WiFi)
- ✗ Identité des utilisateurs connectés
- ✗ Données de localisation des appareils clients
- ✗ Historique des connexions (pas de logs persistants)
- ✗ Données biométriques
- ✗ Données de santé
- ✗ Adresses e-mail ou numéros de téléphone des utilisateurs

---

## 4. Mesures techniques de protection (art. 8 nLPD)

### 4.1 Isolation réseau
- Clients WiFi isolés d'Internet (`FORWARD DROP` iptables)
- Isolation client-à-client (`ap_isolate=1` hostapd)
- DNS forcé vers le nœud local uniquement
- IPv6 désactivé sur toutes les interfaces

### 4.2 Chiffrement
- Alertes : signature Ed25519 + protection anti-rejeu (intégrité et authenticité)
- Liens longue distance : service caché Tor v3 (chiffrement de bout en bout)
- Identités et clés au repos : stockées dans Redb, jamais en clair dans Git ni les logs

### 4.3 Intégrité système
- Système de fichiers racine en lecture seule (`overlayroot`) en production
- Démon unique supervisé par systemd (`Restart=always`), redémarrage automatique
- Binaire statique déployé hors-bande (Ethernet/Tor), jamais depuis le portail web

### 4.4 Logs d'exploitation

Le serveur web (Axum) ne journalise **aucune requête de navigation** : ni adresse IP,
ni adresse MAC, ni URL visitée.

Les logs d'exploitation du démon partent dans **journald** en mémoire volatile
(tmpfs) : ils disparaissent à chaque redémarrage et ne sont jamais écrits sur la
carte SD. Ils ne contiennent aucune donnée personnelle identifiable — seuls le nom
du lieu et les numéros de contact fournis à la configuration figurent dans
`config.json` (jamais dans les logs).

### 4.5 Administration
- Aucune interface d'administration exposée sur le portail public
- Administration via `sos-cli` en accès local (console/SSH) — **zéro `sudo` depuis
  le web**, un seul démon privilégié (surface d'escalade réduite)
- Identités et clés (Ed25519, service Tor v3) stockées dans Redb, jamais en clair
- Réseau WiFi toujours ouvert — accès immédiat au portail sans mot de passe

---

## 5. Droits des personnes concernées (art. 25-27 nLPD)

En raison de l'**absence de collecte de données persistantes**, les droits
d'accès, rectification et effacement (art. 25 nLPD) ne s'appliquent pas
aux utilisateurs du portail captif — aucune donnée les concernant n'est conservée.

Pour les données de configuration (nom du lieu, contacts), le responsable
local du traitement peut les modifier à tout moment via `/admin`.

---

## 6. Transferts internationaux (art. 16 nLPD)

**Aucun transfert international.** Le système fonctionne intégralement
hors ligne. Aucune donnée ne quitte le réseau local sauf via le réseau
LoRa mesh vers d'autres nœuds SOS-GUIDE du même déploiement.

Si la connexion Ethernet est activée pour les mises à jour de contenu :
- Seul le fichier JSON de langue est téléchargé depuis `sos-guide.fr`
- Aucune donnée utilisateur n'est transmise au serveur distant
- La connexion utilise TLS 1.2+ avec vérification du certificat SHA256

---

## 7. Sous-traitants (art. 9 nLPD)

| Sous-traitant | Rôle | Lieu | Garanties |
|---------------|------|------|-----------|
| Raspberry Pi Foundation | Matériel (optionnel) | Royaume-Uni | Décision d'adéquation |
| sos-guide.fr (CDN mises à jour) | Hébergement JSON | France (UE) | RGPD applicable |

Aucun sous-traitant américain. Aucun service cloud. Aucun analytics.

---

## 8. Contact délégué à la protection des données

Pour toute question relative à cette politique :  
**contact@sos-guide.fr**  
Objet : [nLPD] Protection des données SOS-GUIDE

Conformément à l'art. 12 nLPD, toute demande recevra une réponse dans
un délai de 30 jours.

---

## 9. Modifications

Cette politique peut être mise à jour lors des nouvelles versions du logiciel.
La version applicable est celle incluse dans l'image `.img` installée.

Version du document : **3.0 — Juin 2026**  
Intégrité : binaire statique + rootfs en lecture seule (`overlayroot`)

---

*Ce document doit être affiché ou mis à disposition des utilisateurs du portail
captif. Une mention courte « Aucune donnée personnelle collectée — Politique
complète sur demande » est suffisante sur le portail.*
