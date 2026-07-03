/* SOS-GUIDE — i18n partagé (page SOS et, à terme, tout le portail).
 *
 * Texte de SÉCURITÉ : on n'expose que des traductions vérifiées. Toute langue
 * absente (ou clé manquante) retombe sur le FRANÇAIS (règle de repli du projet).
 * Les langues encore non traduites — dont le romanche, exigence PCi-CH —
 * doivent recevoir une relecture humaine avant d'être ajoutées ici.
 *
 * Sans dépendance, sans réseau. Chargé en `<script src=/lib/sos-i18n.js>`.
 */
(function (global) {
  "use strict";
  var FALLBACK = "fr";

  // Libellés de cause, indexés par nom de fil d'AlertType (interop v2.5).
  var CAUSES = {
    fr: {
      PPMS: "⚠️ Plan Particulier de Mise en Sécurité",
      ATTENTAT: "🚨 Attentat / Menace armée",
      NRBC: "☢️ Risque Nucléaire / Radiologique / Biologique / Chimique",
      INCENDIE: "🔥 Incendie",
      CRUE: "🌊 Inondation / Crue",
      SEISME: "🌍 Séisme",
      EVACUATION: "🏃 Évacuation immédiate",
      FIN_ALERTE: "✅ Fin d'alerte — retour à la normale",
      CUSTOM: "📢 Message d'urgence"
    },
    en: {
      PPMS: "⚠️ Shelter-in-place plan",
      ATTENTAT: "🚨 Attack / Armed threat",
      NRBC: "☢️ Nuclear / Radiological / Biological / Chemical hazard",
      INCENDIE: "🔥 Fire",
      CRUE: "🌊 Flood",
      SEISME: "🌍 Earthquake",
      EVACUATION: "🏃 Immediate evacuation",
      FIN_ALERTE: "✅ All clear — back to normal",
      CUSTOM: "📢 Emergency message"
    },
    de: {
      PPMS: "⚠️ Schutzmaßnahmenplan",
      ATTENTAT: "🚨 Anschlag / Bewaffnete Bedrohung",
      NRBC: "☢️ Nukleare / Radiologische / Biologische / Chemische Gefahr",
      INCENDIE: "🔥 Brand",
      CRUE: "🌊 Überschwemmung / Hochwasser",
      SEISME: "🌍 Erdbeben",
      EVACUATION: "🏃 Sofortige Evakuierung",
      FIN_ALERTE: "✅ Entwarnung — zurück zum Normalzustand",
      CUSTOM: "📢 Notfallmeldung"
    },
    it: {
      PPMS: "⚠️ Piano di messa in sicurezza",
      ATTENTAT: "🚨 Attentato / Minaccia armata",
      NRBC: "☢️ Rischio nucleare / radiologico / biologico / chimico",
      INCENDIE: "🔥 Incendio",
      CRUE: "🌊 Alluvione / Piena",
      SEISME: "🌍 Terremoto",
      EVACUATION: "🏃 Evacuazione immediata",
      FIN_ALERTE: "✅ Fine allarme — ritorno alla normalità",
      CUSTOM: "📢 Messaggio di emergenza"
    },
    es: {
      PPMS: "⚠️ Plan de confinamiento de seguridad",
      ATTENTAT: "🚨 Atentado / Amenaza armada",
      NRBC: "☢️ Riesgo nuclear / radiológico / biológico / químico",
      INCENDIE: "🔥 Incendio",
      CRUE: "🌊 Inundación / Crecida",
      SEISME: "🌍 Terremoto",
      EVACUATION: "🏃 Evacuación inmediata",
      FIN_ALERTE: "✅ Fin de la alerta — vuelta a la normalidad",
      CUSTOM: "📢 Mensaje de emergencia"
    },
    pt: {
      PPMS: "⚠️ Plano de abrigo de segurança",
      ATTENTAT: "🚨 Atentado / Ameaça armada",
      NRBC: "☢️ Risco nuclear / radiológico / biológico / químico",
      INCENDIE: "🔥 Incêndio",
      CRUE: "🌊 Inundação / Cheia",
      SEISME: "🌍 Sismo",
      EVACUATION: "🏃 Evacuação imediata",
      FIN_ALERTE: "✅ Fim do alerta — regresso à normalidade",
      CUSTOM: "📢 Mensagem de emergência"
    },
    nl: {
      PPMS: "⚠️ Schuilplan",
      ATTENTAT: "🚨 Aanslag / Gewapende dreiging",
      NRBC: "☢️ Nucleair / Radiologisch / Biologisch / Chemisch gevaar",
      INCENDIE: "🔥 Brand",
      CRUE: "🌊 Overstroming",
      SEISME: "🌍 Aardbeving",
      EVACUATION: "🏃 Onmiddellijke evacuatie",
      FIN_ALERTE: "✅ Einde alarm — terug naar normaal",
      CUSTOM: "📢 Noodbericht"
    }
  };

  // Chrome de la page SOS (consignes génériques + libellés).
  var UI = {
    fr: { alert: "ALERTE", tip1: "Restez calme et mettez-vous à l'abri.", tip2: "Suivez les consignes officielles ci-dessus.", tip3: "Économisez la batterie de votre téléphone.", since: "Depuis", official: "Sources officielles" },
    en: { alert: "ALERT", tip1: "Stay calm and take shelter.", tip2: "Follow the official instructions above.", tip3: "Save your phone's battery.", since: "Since", official: "Official sources" },
    de: { alert: "ALARM", tip1: "Bleiben Sie ruhig und suchen Sie Schutz.", tip2: "Befolgen Sie die obigen amtlichen Anweisungen.", tip3: "Sparen Sie den Akku Ihres Telefons.", since: "Seit", official: "Amtliche Quellen" },
    it: { alert: "ALLARME", tip1: "Mantenete la calma e mettetevi al riparo.", tip2: "Seguite le istruzioni ufficiali qui sopra.", tip3: "Risparmiate la batteria del telefono.", since: "Dal", official: "Fonti ufficiali" },
    es: { alert: "ALERTA", tip1: "Mantenga la calma y póngase a salvo.", tip2: "Siga las instrucciones oficiales anteriores.", tip3: "Ahorre la batería de su teléfono.", since: "Desde", official: "Fuentes oficiales" },
    pt: { alert: "ALERTA", tip1: "Mantenha a calma e abrigue-se.", tip2: "Siga as instruções oficiais acima.", tip3: "Poupe a bateria do seu telemóvel.", since: "Desde", official: "Fontes oficiais" },
    nl: { alert: "ALARM", tip1: "Blijf kalm en zoek beschutting.", tip2: "Volg de officiële instructies hierboven.", tip3: "Spaar de batterij van uw telefoon.", since: "Sinds", official: "Officiële bronnen" }
  };

  function resolveLang(cfgLang) {
    try {
      var s = localStorage.getItem("sos_lang");
      if (s) return s;
    } catch (e) { /* localStorage indisponible : on continue */ }
    if (cfgLang) return cfgLang;
    return (navigator.language || FALLBACK).slice(0, 2).toLowerCase();
  }

  function pick(table, lang, key) {
    var L = table[lang];
    if (L && L[key] != null) return L[key];
    return table[FALLBACK][key];
  }

  function causeLabel(wire, lang) {
    return pick(CAUSES, lang, wire) || CAUSES[FALLBACK][wire] || wire;
  }

  function t(key, lang) {
    return pick(UI, lang, key);
  }

  global.SOSI18N = {
    resolveLang: resolveLang,
    causeLabel: causeLabel,
    t: t,
    translated: Object.keys(UI) // langues réellement traduites (repli fr sinon)
  };
})(window);
