/* SOS-GUIDE — i18n partagé (page SOS, page vitale et, à terme, tout le portail).
 *
 * Texte de SÉCURITÉ : toute langue absente (ou clé manquante) retombe sur le
 * FRANÇAIS (règle de repli du projet).
 * 2026-07-08 : étendu aux 29 langues du portail (romanche inclus, exigence
 * PCi-CH). Traductions générées automatiquement — **RELECTURE HUMAINE EN
 * ATTENTE** (suivi dans ROADMAP.md) ; seules fr/en/de/it/es/pt/nl étaient
 * présentes à l'origine.
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
    },
    pl: {
      PPMS: "⚠️ Plan bezpiecznego schronienia",
      ATTENTAT: "🚨 Zamach / Zagrożenie zbrojne",
      NRBC: "☢️ Zagrożenie jądrowe / radiologiczne / biologiczne / chemiczne",
      INCENDIE: "🔥 Pożar",
      CRUE: "🌊 Powódź",
      SEISME: "🌍 Trzęsienie ziemi",
      EVACUATION: "🏃 Natychmiastowa ewakuacja",
      FIN_ALERTE: "✅ Koniec alarmu — powrót do normalności",
      CUSTOM: "📢 Komunikat alarmowy"
    },
    cs: {
      PPMS: "⚠️ Plán bezpečného ukrytí",
      ATTENTAT: "🚨 Útok / Ozbrojená hrozba",
      NRBC: "☢️ Jaderné / radiologické / biologické / chemické nebezpečí",
      INCENDIE: "🔥 Požár",
      CRUE: "🌊 Povodeň",
      SEISME: "🌍 Zemětřesení",
      EVACUATION: "🏃 Okamžitá evakuace",
      FIN_ALERTE: "✅ Konec poplachu — návrat k normálu",
      CUSTOM: "📢 Nouzová zpráva"
    },
    hu: {
      PPMS: "⚠️ Biztonsági óvintézkedési terv",
      ATTENTAT: "🚨 Merénylet / Fegyveres fenyegetés",
      NRBC: "☢️ Nukleáris / radiológiai / biológiai / vegyi veszély",
      INCENDIE: "🔥 Tűz",
      CRUE: "🌊 Árvíz",
      SEISME: "🌍 Földrengés",
      EVACUATION: "🏃 Azonnali kiürítés",
      FIN_ALERTE: "✅ Riasztás vége — vissza a normál állapotba",
      CUSTOM: "📢 Vészhelyzeti üzenet"
    },
    ro: {
      PPMS: "⚠️ Plan de punere la adăpost",
      ATTENTAT: "🚨 Atentat / Amenințare armată",
      NRBC: "☢️ Risc nuclear / radiologic / biologic / chimic",
      INCENDIE: "🔥 Incendiu",
      CRUE: "🌊 Inundație",
      SEISME: "🌍 Cutremur",
      EVACUATION: "🏃 Evacuare imediată",
      FIN_ALERTE: "✅ Sfârșitul alertei — revenire la normal",
      CUSTOM: "📢 Mesaj de urgență"
    },
    sv: {
      PPMS: "⚠️ Plan för skydd på plats",
      ATTENTAT: "🚨 Attentat / Väpnat hot",
      NRBC: "☢️ Nukleär / radiologisk / biologisk / kemisk fara",
      INCENDIE: "🔥 Brand",
      CRUE: "🌊 Översvämning",
      SEISME: "🌍 Jordbävning",
      EVACUATION: "🏃 Omedelbar evakuering",
      FIN_ALERTE: "✅ Faran över — åter till det normala",
      CUSTOM: "📢 Nödmeddelande"
    },
    da: {
      PPMS: "⚠️ Plan for sikring på stedet",
      ATTENTAT: "🚨 Attentat / Væbnet trussel",
      NRBC: "☢️ Nuklear / radiologisk / biologisk / kemisk fare",
      INCENDIE: "🔥 Brand",
      CRUE: "🌊 Oversvømmelse",
      SEISME: "🌍 Jordskælv",
      EVACUATION: "🏃 Øjeblikkelig evakuering",
      FIN_ALERTE: "✅ Faren er ovre — tilbage til normalen",
      CUSTOM: "📢 Nødmeddelelse"
    },
    no: {
      PPMS: "⚠️ Plan for sikring på stedet",
      ATTENTAT: "🚨 Attentat / Væpnet trussel",
      NRBC: "☢️ Nukleær / radiologisk / biologisk / kjemisk fare",
      INCENDIE: "🔥 Brann",
      CRUE: "🌊 Flom",
      SEISME: "🌍 Jordskjelv",
      EVACUATION: "🏃 Umiddelbar evakuering",
      FIN_ALERTE: "✅ Faren er over — tilbake til det normale",
      CUSTOM: "📢 Nødmelding"
    },
    fi: {
      PPMS: "⚠️ Suojautumissuunnitelma",
      ATTENTAT: "🚨 Isku / Aseellinen uhka",
      NRBC: "☢️ Ydin- / säteily- / biologinen / kemiallinen vaara",
      INCENDIE: "🔥 Tulipalo",
      CRUE: "🌊 Tulva",
      SEISME: "🌍 Maanjäristys",
      EVACUATION: "🏃 Välitön evakuointi",
      FIN_ALERTE: "✅ Hälytys ohi — paluu normaaliin",
      CUSTOM: "📢 Hätäviesti"
    },
    el: {
      PPMS: "⚠️ Σχέδιο ασφαλούς καταφυγής",
      ATTENTAT: "🚨 Επίθεση / Ένοπλη απειλή",
      NRBC: "☢️ Πυρηνικός / ραδιολογικός / βιολογικός / χημικός κίνδυνος",
      INCENDIE: "🔥 Πυρκαγιά",
      CRUE: "🌊 Πλημμύρα",
      SEISME: "🌍 Σεισμός",
      EVACUATION: "🏃 Άμεση εκκένωση",
      FIN_ALERTE: "✅ Λήξη συναγερμού — επιστροφή στην κανονικότητα",
      CUSTOM: "📢 Μήνυμα έκτακτης ανάγκης"
    },
    tr: {
      PPMS: "⚠️ Güvenli sığınma planı",
      ATTENTAT: "🚨 Saldırı / Silahlı tehdit",
      NRBC: "☢️ Nükleer / radyolojik / biyolojik / kimyasal tehlike",
      INCENDIE: "🔥 Yangın",
      CRUE: "🌊 Sel / Taşkın",
      SEISME: "🌍 Deprem",
      EVACUATION: "🏃 Derhal tahliye",
      FIN_ALERTE: "✅ Alarm sona erdi — normale dönüş",
      CUSTOM: "📢 Acil durum mesajı"
    },
    ru: {
      PPMS: "⚠️ План укрытия на месте",
      ATTENTAT: "🚨 Теракт / Вооружённая угроза",
      NRBC: "☢️ Ядерная / радиологическая / биологическая / химическая опасность",
      INCENDIE: "🔥 Пожар",
      CRUE: "🌊 Наводнение",
      SEISME: "🌍 Землетрясение",
      EVACUATION: "🏃 Немедленная эвакуация",
      FIN_ALERTE: "✅ Отбой тревоги — возвращение к норме",
      CUSTOM: "📢 Экстренное сообщение"
    },
    uk: {
      PPMS: "⚠️ План укриття на місці",
      ATTENTAT: "🚨 Теракт / Збройна загроза",
      NRBC: "☢️ Ядерна / радіологічна / біологічна / хімічна небезпека",
      INCENDIE: "🔥 Пожежа",
      CRUE: "🌊 Повінь",
      SEISME: "🌍 Землетрус",
      EVACUATION: "🏃 Негайна евакуація",
      FIN_ALERTE: "✅ Відбій тривоги — повернення до норми",
      CUSTOM: "📢 Екстрене повідомлення"
    },
    ar: {
      PPMS: "⚠️ خطة الاحتماء في المكان",
      ATTENTAT: "🚨 هجوم / تهديد مسلح",
      NRBC: "☢️ خطر نووي / إشعاعي / بيولوجي / كيميائي",
      INCENDIE: "🔥 حريق",
      CRUE: "🌊 فيضان",
      SEISME: "🌍 زلزال",
      EVACUATION: "🏃 إخلاء فوري",
      FIN_ALERTE: "✅ انتهاء الإنذار — العودة إلى الوضع الطبيعي",
      CUSTOM: "📢 رسالة طوارئ"
    },
    he: {
      PPMS: "⚠️ תוכנית התמגנות במקום",
      ATTENTAT: "🚨 פיגוע / איום חמוש",
      NRBC: "☢️ סכנה גרעינית / רדיולוגית / ביולוגית / כימית",
      INCENDIE: "🔥 שרפה",
      CRUE: "🌊 הצפה / שיטפון",
      SEISME: "🌍 רעידת אדמה",
      EVACUATION: "🏃 פינוי מיידי",
      FIN_ALERTE: "✅ סיום ההתרעה — חזרה לשגרה",
      CUSTOM: "📢 הודעת חירום"
    },
    fa: {
      PPMS: "⚠️ طرح پناه‌گیری در محل",
      ATTENTAT: "🚨 حمله / تهدید مسلحانه",
      NRBC: "☢️ خطر هسته‌ای / پرتوی / زیستی / شیمیایی",
      INCENDIE: "🔥 آتش‌سوزی",
      CRUE: "🌊 سیل",
      SEISME: "🌍 زمین‌لرزه",
      EVACUATION: "🏃 تخلیه فوری",
      FIN_ALERTE: "✅ پایان هشدار — بازگشت به حالت عادی",
      CUSTOM: "📢 پیام اضطراری"
    },
    hi: {
      PPMS: "⚠️ सुरक्षित आश्रय योजना",
      ATTENTAT: "🚨 हमला / सशस्त्र ख़तरा",
      NRBC: "☢️ परमाणु / विकिरण / जैविक / रासायनिक ख़तरा",
      INCENDIE: "🔥 आग",
      CRUE: "🌊 बाढ़",
      SEISME: "🌍 भूकंप",
      EVACUATION: "🏃 तुरंत निकासी",
      FIN_ALERTE: "✅ चेतावनी समाप्त — स्थिति सामान्य",
      CUSTOM: "📢 आपातकालीन संदेश"
    },
    zh: {
      PPMS: "⚠️ 就地避险方案",
      ATTENTAT: "🚨 袭击 / 武装威胁",
      NRBC: "☢️ 核 / 辐射 / 生物 / 化学危险",
      INCENDIE: "🔥 火灾",
      CRUE: "🌊 洪水",
      SEISME: "🌍 地震",
      EVACUATION: "🏃 立即疏散",
      FIN_ALERTE: "✅ 警报解除——恢复正常",
      CUSTOM: "📢 紧急消息"
    },
    ja: {
      PPMS: "⚠️ 屋内退避計画",
      ATTENTAT: "🚨 襲撃 / 武装した脅威",
      NRBC: "☢️ 核・放射線・生物・化学の危険",
      INCENDIE: "🔥 火災",
      CRUE: "🌊 洪水",
      SEISME: "🌍 地震",
      EVACUATION: "🏃 直ちに避難",
      FIN_ALERTE: "✅ 警報解除 — 平常に戻りました",
      CUSTOM: "📢 緊急メッセージ"
    },
    ko: {
      PPMS: "⚠️ 현장 실내 대피 계획",
      ATTENTAT: "🚨 테러 / 무장 위협",
      NRBC: "☢️ 핵 / 방사능 / 생물학 / 화학 위험",
      INCENDIE: "🔥 화재",
      CRUE: "🌊 홍수",
      SEISME: "🌍 지진",
      EVACUATION: "🏃 즉시 대피",
      FIN_ALERTE: "✅ 경보 해제 — 정상 복귀",
      CUSTOM: "📢 긴급 메시지"
    },
    th: {
      PPMS: "⚠️ แผนหลบภัยในสถานที่",
      ATTENTAT: "🚨 การโจมตี / ภัยคุกคามด้วยอาวุธ",
      NRBC: "☢️ อันตรายนิวเคลียร์ / รังสี / ชีวภาพ / เคมี",
      INCENDIE: "🔥 ไฟไหม้",
      CRUE: "🌊 น้ำท่วม",
      SEISME: "🌍 แผ่นดินไหว",
      EVACUATION: "🏃 อพยพทันที",
      FIN_ALERTE: "✅ สิ้นสุดการเตือนภัย — กลับสู่ภาวะปกติ",
      CUSTOM: "📢 ข้อความฉุกเฉิน"
    },
    vi: {
      PPMS: "⚠️ Kế hoạch trú ẩn tại chỗ",
      ATTENTAT: "🚨 Tấn công / Đe dọa vũ trang",
      NRBC: "☢️ Nguy cơ hạt nhân / phóng xạ / sinh học / hóa học",
      INCENDIE: "🔥 Hỏa hoạn",
      CRUE: "🌊 Lũ lụt",
      SEISME: "🌍 Động đất",
      EVACUATION: "🏃 Sơ tán ngay lập tức",
      FIN_ALERTE: "✅ Hết báo động — trở lại bình thường",
      CUSTOM: "📢 Thông báo khẩn cấp"
    },
    rm: {
      PPMS: "⚠️ Plan da protecziun sin plaz",
      ATTENTAT: "🚨 Attentat / Smanatscha armada",
      NRBC: "☢️ Privel nuclear / radiologic / biologic / chemic",
      INCENDIE: "🔥 Incendi",
      CRUE: "🌊 Inundaziun",
      SEISME: "🌍 Terratrembel",
      EVACUATION: "🏃 Evacuaziun immediata",
      FIN_ALERTE: "✅ Fin da l'alarm — enavos al normal",
      CUSTOM: "📢 Messadi d'urgenza"
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
    nl: { alert: "ALARM", tip1: "Blijf kalm en zoek beschutting.", tip2: "Volg de officiële instructies hierboven.", tip3: "Spaar de batterij van uw telefoon.", since: "Sinds", official: "Officiële bronnen" },
    pl: { alert: "ALARM", tip1: "Zachowaj spokój i schroń się.", tip2: "Postępuj zgodnie z powyższymi oficjalnymi zaleceniami.", tip3: "Oszczędzaj baterię telefonu.", since: "Od", official: "Źródła oficjalne" },
    cs: { alert: "POPLACH", tip1: "Zachovejte klid a ukryjte se.", tip2: "Řiďte se výše uvedenými oficiálními pokyny.", tip3: "Šetřete baterii telefonu.", since: "Od", official: "Oficiální zdroje" },
    hu: { alert: "RIASZTÁS", tip1: "Őrizze meg nyugalmát, és keressen menedéket.", tip2: "Kövesse a fenti hivatalos utasításokat.", tip3: "Kímélje telefonja akkumulátorát.", since: "Kezdete:", official: "Hivatalos források" },
    ro: { alert: "ALERTĂ", tip1: "Păstrați-vă calmul și adăpostiți-vă.", tip2: "Urmați instrucțiunile oficiale de mai sus.", tip3: "Economisiți bateria telefonului.", since: "Din", official: "Surse oficiale" },
    sv: { alert: "LARM", tip1: "Håll dig lugn och sök skydd.", tip2: "Följ de officiella anvisningarna ovan.", tip3: "Spara på telefonens batteri.", since: "Sedan", official: "Officiella källor" },
    da: { alert: "ALARM", tip1: "Bevar roen og søg ly.", tip2: "Følg de officielle anvisninger ovenfor.", tip3: "Spar på telefonens batteri.", since: "Siden", official: "Officielle kilder" },
    no: { alert: "ALARM", tip1: "Hold deg rolig og søk ly.", tip2: "Følg de offisielle instruksene ovenfor.", tip3: "Spar på telefonbatteriet.", since: "Siden", official: "Offisielle kilder" },
    fi: { alert: "HÄLYTYS", tip1: "Pysy rauhallisena ja suojaudu.", tip2: "Noudata yllä olevia virallisia ohjeita.", tip3: "Säästä puhelimesi akkua.", since: "Alkaen", official: "Viralliset lähteet" },
    el: { alert: "ΣΥΝΑΓΕΡΜΟΣ", tip1: "Μείνετε ψύχραιμοι και καλυφθείτε.", tip2: "Ακολουθήστε τις παραπάνω επίσημες οδηγίες.", tip3: "Εξοικονομήστε την μπαταρία του τηλεφώνου σας.", since: "Από", official: "Επίσημες πηγές" },
    tr: { alert: "ALARM", tip1: "Sakin olun ve güvenli bir yere sığının.", tip2: "Yukarıdaki resmî talimatlara uyun.", tip3: "Telefonunuzun şarjını idareli kullanın.", since: "Başlangıç:", official: "Resmî kaynaklar" },
    ru: { alert: "ТРЕВОГА", tip1: "Сохраняйте спокойствие и укройтесь.", tip2: "Следуйте официальным указаниям выше.", tip3: "Экономьте заряд телефона.", since: "С", official: "Официальные источники" },
    uk: { alert: "ТРИВОГА", tip1: "Зберігайте спокій і сховайтеся в укритті.", tip2: "Дотримуйтесь офіційних вказівок вище.", tip3: "Заощаджуйте заряд телефона.", since: "З", official: "Офіційні джерела" },
    ar: { alert: "إنذار", tip1: "حافظ على هدوئك واحتمِ.", tip2: "اتبع التعليمات الرسمية أعلاه.", tip3: "وفّر بطارية هاتفك.", since: "منذ", official: "مصادر رسمية" },
    he: { alert: "התרעה", tip1: "שמרו על קור רוח ותפסו מחסה.", tip2: "פעלו לפי ההנחיות הרשמיות שלמעלה.", tip3: "חסכו בסוללת הטלפון.", since: "מאז", official: "מקורות רשמיים" },
    fa: { alert: "هشدار", tip1: "آرامش خود را حفظ کنید و پناه بگیرید.", tip2: "از دستورالعمل‌های رسمی بالا پیروی کنید.", tip3: "در مصرف باتری تلفن صرفه‌جویی کنید.", since: "از", official: "منابع رسمی" },
    hi: { alert: "चेतावनी", tip1: "शांत रहें और सुरक्षित स्थान पर जाएँ।", tip2: "ऊपर दिए गए आधिकारिक निर्देशों का पालन करें।", tip3: "अपने फ़ोन की बैटरी बचाएँ।", since: "प्रारंभ:", official: "आधिकारिक स्रोत" },
    zh: { alert: "警报", tip1: "保持冷静，就近避险。", tip2: "请遵循上方官方指示。", tip3: "节省手机电量。", since: "开始时间：", official: "官方来源" },
    ja: { alert: "警報", tip1: "落ち着いて身を守ってください。", tip2: "上記の公式指示に従ってください。", tip3: "携帯電話の電池を節約してください。", since: "発生時刻:", official: "公式情報源" },
    ko: { alert: "경보", tip1: "침착함을 유지하고 안전한 곳으로 피하세요.", tip2: "위의 공식 지침을 따르세요.", tip3: "휴대전화 배터리를 아끼세요.", since: "시작:", official: "공식 출처" },
    th: { alert: "เตือนภัย", tip1: "ตั้งสติและหาที่หลบภัย", tip2: "ปฏิบัติตามคำแนะนำทางการด้านบน", tip3: "ประหยัดแบตเตอรี่โทรศัพท์ของคุณ", since: "ตั้งแต่", official: "แหล่งข้อมูลทางการ" },
    vi: { alert: "BÁO ĐỘNG", tip1: "Giữ bình tĩnh và tìm nơi trú ẩn.", tip2: "Làm theo các chỉ dẫn chính thức ở trên.", tip3: "Tiết kiệm pin điện thoại.", since: "Từ", official: "Nguồn chính thức" },
    rm: { alert: "ALARM", tip1: "Restai calm e tschertgai protecziun.", tip2: "Suondai las instrucziuns uffizialas survart.", tip3: "Spargnai la battaria da voss telefon.", since: "Dapi", official: "Funtaunas uffizialas" }
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
