#!/usr/bin/env python3
"""Générateur de la page d'audit locale SOS-GUIDE.

Lit la feuille de route (docs/ROADMAP.md) et le registre d'erreurs
(audit-control/ERRORS.md), produit une page autonome `audit.html` ouverte
sur le PC de dev (Arch). La page liste :
  - les étapes du projet (cases à cocher pour celles à implémenter) ;
  - les erreurs détectées (cases à cocher pour celles à corriger) ;
  - les corrections déjà appliquées (affichage seul) ;
et génère le prompt de la prochaine session Claude à partir des cases cochées.

Aucune dépendance externe, aucune donnée envoyée nulle part. Hors de l'img produit.
"""
import os
import re
import html
import datetime

HERE = os.path.dirname(os.path.abspath(__file__))
ROADMAP = os.path.join(HERE, "..", "docs", "ROADMAP.md")
ERRORS = os.path.join(HERE, "ERRORS.md")
OUT = os.path.join(HERE, "audit.html")


def read(path):
    try:
        with open(path, encoding="utf-8") as f:
            return f.read()
    except OSError:
        return ""


def parse_steps(md):
    """Étapes au format liste de tâches : `- [ ] ...` / `- [x] ...`."""
    steps = []
    for m in re.finditer(r"^- \[([ xX])\]\s+(.+)$", md, re.M):
        steps.append((m.group(1).lower() == "x", m.group(2).strip()))
    return steps


def section(md, title):
    """Renvoie le corps d'une section `## title` jusqu'au prochain `## `."""
    m = re.search(r"^## .*" + re.escape(title) + r".*$", md, re.M)
    if not m:
        return ""
    start = m.end()
    nxt = re.search(r"^## ", md[start:], re.M)
    return md[start:start + nxt.start()] if nxt else md[start:]


def split_section(md, title):
    """Comme `section`, mais renvoie `(corps, md_sans_la_section)` — pour
    extraire une section et l'exclure du reste (ex. durcissement vs étapes)."""
    m = re.search(r"^## .*" + re.escape(title) + r".*$", md, re.M)
    if not m:
        return "", md
    body_start = m.end()
    nxt = re.search(r"^## ", md[body_start:], re.M)
    end = body_start + nxt.start() if nxt else len(md)
    return md[body_start:end], md[:m.start()] + md[end:]


def parse_errors(body):
    """Entrées curées `### ERR-... — titre` + captures brutes `- \\`...\\` — échec`."""
    out = []
    for m in re.finditer(r"^#{3,4}\s+(.+)$", body, re.M):
        out.append(m.group(1).strip())
    for m in re.finditer(r"^- `[^`]+`\s+—\s+(échec.+)$", body, re.M):
        out.append(m.group(1).strip())
    return out


def esc(s):
    return html.escape(s, quote=True)


def clean(s):
    """Allège le markdown inline d'un libellé (gras `**`, code `` ` ``) pour un
    rendu lisible dans la page d'audit, puis échappe le HTML."""
    s = s.replace("**", "").replace("`", "")
    return esc(s)


def main():
    rm = read(ROADMAP)
    er = read(ERRORS)
    # La section « Durcissement appliance » est rendue à part (vignette de
    # statut) et exclue du décompte des étapes générales.
    hard_body, rm_rest = split_section(rm, "Durcissement appliance")
    hardening = parse_steps(hard_body)
    steps = parse_steps(rm_rest)
    actives = parse_errors(section(er, "Actives") + section(er, "Inbox"))
    resolved = parse_errors(section(er, "Résolues"))

    done = sum(1 for d, _ in steps if d)
    total = len(steps)
    pct = round(done / total * 100) if total else 0

    steps_html = ""
    for d, label in steps:
        if d:
            steps_html += (
                f'<li class="done"><span class="chk done">✓</span>'
                f'<span>{clean(label)}</span></li>'
            )
        else:
            steps_html += (
                f'<li><label><input type="checkbox" class="step-cb" '
                f'data-label="{clean(label)}"><span>{clean(label)}</span></label></li>'
            )
    if not steps:
        steps_html = '<li class="empty">Aucune étape dans ROADMAP.md.</li>'

    err_html = ""
    for label in actives:
        err_html += (
            f'<li><label><input type="checkbox" class="err-cb" '
            f'data-label="{clean(label)}"><span>{clean(label)}</span></label></li>'
        )
    if not actives:
        err_html = '<li class="empty">Aucune erreur active. 🎉</li>'

    corr_html = "".join(f"<li>{clean(c)}</li>" for c in resolved) or \
        '<li class="empty">Aucune correction enregistrée.</li>'

    # Vignette de durcissement : statut en lecture seule (✓ acquis / ⚠ à faire).
    hard_html = ""
    for ok, label in hardening:
        cls, badge = ("ok", "✓") if ok else ("todo", "⚠")
        hard_html += (
            f'<li class="{cls}"><span class="badge {cls}">{badge}</span>'
            f'<span>{clean(label)}</span></li>'
        )
    hard_done = sum(1 for ok, _ in hardening if ok)
    hard_total = len(hardening)
    if hardening:
        full = hard_done == hard_total
        hard_seal = "🔒 SOSDATA ro" if full else "⚠ partiel"
        hard_seal_cls = "" if full else "partial"
    else:
        hard_seal = ""
        hard_seal_cls = ""

    stamp = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    page = (
        TEMPLATE
        .replace("__STEPS__", steps_html)
        .replace("__ERRORS__", err_html)
        .replace("__CORR__", corr_html)
        .replace("__HARDENING__", hard_html or
                 '<li class="empty">Aucun item de durcissement.</li>')
        .replace("__HARDDONE__", str(hard_done))
        .replace("__HARDTOTAL__", str(hard_total))
        .replace("__HARDSEAL__", hard_seal)
        .replace("__HARDSEALCLS__", hard_seal_cls)
        .replace("__PCT__", str(pct))
        .replace("__DONE__", str(done))
        .replace("__TOTAL__", str(total))
        .replace("__NERR__", str(len(actives)))
        .replace("__DATE__", stamp)
    )
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(page)
    print(OUT)


TEMPLATE = r"""<!DOCTYPE html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>SOS-GUIDE — Audit & Contrôle</title>
<style>
  :root{--bg:#0e1116;--panel:#161b22;--border:#2a3340;--txt:#e6edf3;--muted:#8b97a7;
    --accent:#ff5a36;--ok:#3fb950;--warn:#d29922;--bar:#21304a}
  *{box-sizing:border-box}
  body{margin:0;background:var(--bg);color:var(--txt);
    font:15px/1.55 system-ui,-apple-system,Segoe UI,Roboto,sans-serif}
  header{padding:18px 22px;border-bottom:1px solid var(--border);background:var(--panel);
    display:flex;align-items:baseline;gap:14px;flex-wrap:wrap}
  header .logo{font-weight:800;color:var(--accent);font-size:20px;letter-spacing:.5px}
  header .date{color:var(--muted);font-size:12px;margin-left:auto}
  main{max-width:1000px;margin:0 auto;padding:22px 22px 220px}
  h2{font-size:13px;text-transform:uppercase;letter-spacing:1px;color:var(--muted);
    margin:30px 0 12px}
  .overview{display:flex;align-items:center;gap:18px;background:var(--panel);
    border:1px solid var(--border);border-radius:12px;padding:16px}
  .overview .pct{font-size:40px;font-weight:800}
  .overview .meta{color:var(--muted)}
  .bar{flex:1;height:12px;border-radius:6px;background:var(--bar);overflow:hidden}
  .bar>i{display:block;height:100%;background:var(--ok);width:__PCT__%}
  ul{list-style:none;margin:0;padding:0;background:var(--panel);border:1px solid var(--border);
    border-radius:12px;overflow:hidden}
  li{display:flex;gap:10px;align-items:flex-start;padding:9px 14px;
    border-top:1px solid var(--border);font-size:14px}
  ul li:first-child{border-top:0}
  li.empty{color:var(--muted);justify-content:center}
  li label{display:flex;gap:10px;align-items:flex-start;cursor:pointer;width:100%}
  li input{margin-top:3px;width:16px;height:16px;accent-color:var(--accent);flex:none}
  li.done span{color:var(--muted);text-decoration:line-through}
  .chk{flex:none;width:18px;height:18px;border-radius:5px;display:grid;place-items:center;
    font-size:12px;background:var(--ok);color:var(--bg)}
  .err li input{accent-color:var(--warn)}
  .hard-head{display:flex;align-items:center;gap:12px;margin:30px 0 12px}
  .hard-head h2{margin:0}
  .seal{font-size:12px;font-weight:700;padding:3px 10px;border-radius:999px;
    background:rgba(63,185,80,.15);color:var(--ok);border:1px solid var(--ok)}
  .seal.partial{background:rgba(210,153,34,.15);color:var(--warn);border-color:var(--warn)}
  ul.hard li{color:var(--muted)}
  ul.hard li.ok span:last-child{color:var(--txt)}
  .badge{flex:none;width:20px;height:20px;border-radius:6px;display:grid;
    place-items:center;font-size:12px;font-weight:700}
  .badge.ok{background:var(--ok);color:var(--bg)}
  .badge.todo{background:var(--warn);color:var(--bg)}
  .panel-foot{position:fixed;left:0;right:0;bottom:0;background:var(--panel);
    border-top:1px solid var(--border);padding:14px 22px}
  .panel-foot .inner{max-width:1000px;margin:0 auto;display:flex;gap:12px;align-items:flex-start}
  textarea{flex:1;min-height:90px;background:var(--bg);color:var(--txt);
    border:1px solid var(--border);border-radius:10px;padding:10px;font:13px/1.5 monospace;resize:vertical}
  .btns{display:flex;flex-direction:column;gap:8px;flex:none}
  button{background:var(--accent);color:#fff;border:none;border-radius:9px;padding:10px 16px;
    font-weight:600;font-size:14px;cursor:pointer;white-space:nowrap}
  button.sec{background:var(--bar);color:var(--txt)}
  button:active{transform:scale(.97)}
  .count{font-size:12px;color:var(--muted);text-align:center}
</style>
</head>
<body>
<header>
  <span class="logo">SOS-GUIDE</span><span>Audit &amp; Contrôle</span>
  <span class="date">généré le __DATE__ · local, hors-ligne</span>
</header>
<main>
  <div class="overview">
    <div class="pct">__PCT__%</div>
    <div style="flex:1">
      <div class="meta">__DONE__ / __TOTAL__ étapes réalisées · __NERR__ erreur(s) active(s)</div>
      <div class="bar"><i></i></div>
    </div>
  </div>

  <div class="hard-head">
    <h2>Durcissement appliance — SOSDATA en lecture seule</h2>
    <span class="seal __HARDSEALCLS__">__HARDSEAL__ · __HARDDONE__/__HARDTOTAL__</span>
  </div>
  <ul id="hardening" class="hard">__HARDENING__</ul>

  <h2>Étapes du projet — cocher celles à implémenter</h2>
  <ul id="steps">__STEPS__</ul>

  <h2>Erreurs détectées — cocher celles à corriger</h2>
  <ul id="errors" class="err">__ERRORS__</ul>

  <h2>Corrections appliquées</h2>
  <ul id="corr">__CORR__</ul>
</main>

<div class="panel-foot">
  <div class="inner">
    <textarea id="out" readonly placeholder="Coche des étapes/erreurs puis « Générer le prompt »…"></textarea>
    <div class="btns">
      <button onclick="genPrompt()">Générer le prompt</button>
      <button id="copybtn" class="sec" onclick="copyPrompt()">Copier</button>
      <div class="count" id="count">0 sélection(s)</div>
    </div>
  </div>
</div>

<script>
function selected(cls){ return [...document.querySelectorAll(cls+':checked')].map(c=>c.dataset.label); }
function refresh(){
  const n = selected('.step-cb').length + selected('.err-cb').length;
  document.getElementById('count').textContent = n + ' sélection(s)';
}
document.addEventListener('change', e => { if(e.target.matches('input[type=checkbox]')) refresh(); });
function genPrompt(){
  const steps = selected('.step-cb'), errs = selected('.err-cb');
  let p = 'Session SOS-GUIDE — feuille de route.\n\n';
  if(steps.length) p += 'Étapes à implémenter :\n' + steps.map(s=>'- '+s).join('\n') + '\n\n';
  if(errs.length) p += 'Erreurs à corriger :\n' + errs.map(s=>'- '+s).join('\n') + '\n\n';
  if(!steps.length && !errs.length) p += '(Coche au moins une étape ou une erreur ci-dessus.)\n\n';
  p += 'Respecte CLAUDE.md (fiabilité > simplicité > sécurité > sobriété > perf). ';
  p += 'À la fin : mets à jour docs/ROADMAP.md, docs/CHANGELOG.md et ERRORS.md.';
  document.getElementById('out').value = p;
}
function copyPrompt(){
  const t = document.getElementById('out');
  if(!t.value) genPrompt();
  t.select();
  const done = () => { const b=document.getElementById('copybtn'); b.textContent='✓ Copié'; setTimeout(()=>b.textContent='Copier',1500); };
  if(navigator.clipboard) navigator.clipboard.writeText(t.value).then(done, ()=>{document.execCommand('copy');done();});
  else { document.execCommand('copy'); done(); }
}
</script>
</body>
</html>
"""

if __name__ == "__main__":
    main()
