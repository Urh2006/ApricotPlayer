# Izvedbeni plan macOS porta Rust 2.0

Datum: 10. oktober 2026.
Izhodišče: veja `rust-2.0` na 2898684 (2.0.0-beta.1).
Pogodba: `docs/MACOS_PORT_PLAN.md` (feature parity manifest za macOS),
`docs/RUST_PARITY_MANIFEST.md`, `docs/SPOTIFY_PARITY_MANIFEST.md`.
Ta dokument nadomešča samo Python/wxPython izvedbeno zaporedje iz
`MACOS_PORT_PLAN.md`. Vsebina manifesta ostaja nespremenjena.

Cilj: macOS izdaja, ki je funkcionalno 1:1 z Windows Rust klientom, vključno s
Spotifyjem. VoiceOver nadomesti NVDA, Ctrl postane Command, Alt postane Option,
vse ostalo (vrstni red Tab, fokus, Escape, oglasi, meniji, besedila, podatki)
ostane enako.

## 1. Stanje okolja na Macu

- MacBook z Apple Silicon (arm64), macOS 27.0.1.
- Nameščeno: Command Line Tools (brez polnega Xcoda), Homebrew, git, rustup z
  Rust 1.98.0 iz `rust-toolchain.toml`, mpv 0.41 z `libmpv.2.dylib`, gh,
  mingw-w64 za preverjanje Windows gradnje.
- Polni Xcode trenutno ni potreben. AppKit SDK je v Command Line Tools. Xcode
  (zaradi Accessibility Inspectorja) je koristen, ni pa pogoj.

## 2. Ugotovitve iz obstoječe kode

1. **Na macOS se danes prevedejo samo nevtralni crati.** `apricot-core`,
   `apricot-storage`, `apricot-media` in `apricot-playback` se prevedejo, njihovi
   testi gredo skozi. `apricot-updater` ima dva testa za PowerShell skripte, ki na
   macOS padeta.
2. **`apricot-platform` brezpogojno odvisen od crata `windows`.** Zaradi tega se
   na macOS ne prevedejo `apricot-platform`, `apricot-spotify`, `apricot-app` in
   aplikacija. To je prva ovira.
3. **Windows gradnjo lahko preverjam z Maca.** `cargo check --workspace
   --all-targets --target x86_64-pc-windows-gnu` gre skozi. Teste in NVDA na
   Windows pa lahko požene samo Urh na Windows računalniku ali CI.
4. **`win32.rs` (19.100 vrstic) meša UI in aplikacijsko logiko.** `WindowState`
   poleg HWND kontrol hrani čakajoča YouTube dela, predpomnilnik stream URL-jev,
   prefetch, razreševanje predvajanja, prenose in njihov napredek, pretvorbe,
   posodobitve, BPM, poglavja, sorodne videe, podcaste, AudioVault in Spotify
   stanje. Če bi AppKit UI napisal ob tem, bi podvojil približno polovico te
   datoteke in Windows ter macOS bi se začela razhajati. To je v nasprotju s
   pravilom "ena skupna koda" iz `MACOS_PORT_PLAN.md`. Zato je izločitev te logike
   v skupni crate največji in najbolj tvegan del porta, in mora biti narejena
   pred zasloni na Macu.
5. **libmpv je že pravi temelj.** Nalaganje gre prek `libloading`; na Macu je
   treba dodati iskanje `libmpv.2.dylib` in `wid` iz `NSView`. Spotify zvok gre
   iz LibreSpot sinka v libmpv, zato se prenese brez posebne macOS zvočne poti.
6. **Platformne storitve so razpršene.** DPAPI (AudioVault, Spotify računi),
   named pipes, registracija v registru, enojna instanca, poti `%APPDATA%`,
   zastavice procesov in PowerShell updater so v posameznih modulih z `cfg`.
   Potrebujejo eno platformsko mejo z Windows in macOS izvedbo.

## 3. Arhitektura macOS porta

```text
apricot-core, apricot-storage, apricot-media, apricot-playback   (nespremenjeno)
apricot-platform        Windows in macOS izvedbe za skupne storitve
apricot-app             + skupni "shell" kontroler, izločen iz win32.rs
apricot-ui-windows      tanek Win32 renderer
apricot-ui-macos        nov, AppKit prek objc2, enak model pogledov in dejanj
apps/apricot-player     izbere UI glede na cilj
```

- UI: AppKit prek `objc2` in `objc2-app-kit`, nativne kontrole (NSTableView,
  NSButton, NSTextField, NSPopUpButton, NSSlider, NSMenu), da VoiceOver dobi
  prave vloge, imena in vrednosti. Brez lastnega risanja.
- Oglasi: `NSAccessibilityPostNotification` z `announcementRequested`, ena pot,
  ista pravila podvajanja kot pri NVDA. Brez `say`.
- Bližnjice: shranjene logične (Ctrl ostane v JSON), prikazane in obdelane kot
  Command. Ozadno predvajanje privzeto Command+Shift+Space. Kontekstni meni na
  Shift+F10 in na VoiceOverjev ukaz (VO+Shift+M).
- Podatki: `~/Library/Application Support/<identiteta>`, predpomnilnik v
  `~/Library/Caches`, dnevniki v `~/Library/Logs`; identiteta beta/stabilna enako
  kot na Windows. Skrivnosti v Keychainu.
- Paket: `.app` z libmpv in odvisnostmi, FFmpeg, yt-dlp, Node, ad-hoc podpis,
  DMG `ApricotPlayer-macOS-arm64.dmg`.

## 4. Delovne enote

Vsaka enota se konča z: `cargo build`, `cargo test` in `cargo clippy` na macOS,
`cargo check` in `cargo clippy` za `x86_64-pc-windows-gnu`, ročnim preizkusom
(VoiceOver ali Windows NVDA, kjer se je spremenil skupni del) in vpisom v dnevnik
spodaj. Po vsaki enoti se ustavim in počakam na "nadaljuj".

### Del A: skupna osnova brez vidnih sprememb na Windows

- **M0. Gradnja celotnega delovnega prostora na macOS.** `windows` postane
  odvisnost samo za `cfg(windows)`, Windows moduli v `apricot-platform` in
  `apricot-spotify` dobijo `cfg`, PowerShell testi updaterja tečejo samo na
  Windows, `apricot-ui-windows` je na macOS prazen. Rezultat: `cargo test
  --workspace` gre skozi na macOS, Windows koda nespremenjena.
- **M1. Platformna meja.** En vmesnik za poti, skrivnosti (DPAPI ali Keychain),
  enojno instanco in posredovanje datotek, odpiranje URL-jev in map, odložišče,
  zagon ob prijavi, povezave datotek, obvestila in zaganjanje pomožnih procesov.
  Windows izvedba je dobesedno premaknjena obstoječa koda. macOS izvedba za poti,
  Keychain in odpiranje je narejena takoj, ostalo v M14.
- **M2. AppKit in VoiceOver spike.** Majhno okno: seznam z začetnicami, gumbi,
  polje za iskanje, potrditveno polje, drsnik, meni, kontekstni meni na Shift+F10
  in VO+Shift+M, oglas, obnova fokusa po dialogu, ter libmpv zvok in video v
  `NSView`. Vrata: Urh potrdi z VoiceOverjem, da so imena, vloge in oglasi
  pravilni, in da video deluje. Če AppKit prek `objc2` ne izpolni pogodbe, se
  ustavim in predlagam alternativo.
- **M3. Skupni koordinator `apricot-shell`.** Logika, ki je danes v `win32.rs`
  (razreševanje in začetek predvajanja, delo v ozadju, prenosi, pogovorna okna v
  pravem vrstnem redu, besedila stanja), se prenese v crate brez Win32 in brez
  AppKita. UI ga kliče in mu ponudi vmesnik `ShellUi` za vrstico stanja,
  sporočila, potrditve, izbire, datotečna okna, obvestila in osvežitev pogledov.
  Prenos poteka po sklopih skupaj z macOS zasloni v enotah M4 do M14, vsak sklop
  z enotskimi testi na Macu.

Sprememba (11. 10. 2026): prvotni M2 je predvideval, da se ta logika najprej
izloči iz `win32.rs` in Windows takoj preklopi nanjo. Ker Urh Windows testira
šele na koncu, bi to pomenilo dolgo obdobje predelanega, a nepreizkušenega
Windows vmesnika. Zato macOS vmesnik prvi uporablja `apricot-shell`, `win32.rs`
pa ostane nespremenjen do enote W1, ko Urh lahko sproti preizkuša z NVDA.

### Del C: zasloni in funkcije na macOS

- **M4. Predvajalnik.** Zvok in vgrajen video (render API iz M2), celozaslonski
  način, izhodne naprave, glasnost, hitrost, višina tona, EQ, ReplayGain, BPM,
  iskanje po času, zadrževane tipke, ozadno predvajanje, oglasi predvajalnika;
  odpiranje datoteke in mape iz glavnega menija kot prvi vir. Ogrodje
  (aplikacija, glavno okno, menijska vrstica, izbira jezika, preslikava
  bližnjic) je bilo narejeno že v M3; Action Finder pride z nastavitvami (M10).
- **M5.** (združeno v M4)
- **M6. Iskanje in odkrivanje.** YouTube, SoundCloud, Trending, kanali,
  playlisti, Shorts, dinamično nalaganje, stolpci rezultatov, neposredna povezava.
- **M7. Knjižnica.** Priljubljeni, zgodovina, zaznamki, playlisti, vrsta
  predvajanja, center obvestil, naročnine, mapa in datoteka.
- **M8. Podcasti in RSS.**
- **M9. Prenosi in pretvorbe.** Zaslon trenutnih prenosov, okno napredka,
  pretvornik datotek in map, izvoz izseka in urejene kopije.
- **M10. Nastavitve.** Vsi razdelki, urejevalnik bližnjic s Command in Option,
  prilagajanje glavnega menija, ponastavitve.
- **M11. Dialogi predvajalnika.** Podrobnosti, poglavja, prepis, besedila,
  komentarji, zaznamki, izenačevalnik, način urejanja.
- **M12. AudioVault in piškotki.** Keychain za geslo; piškotki iz Safarija,
  Chroma (Keychain "Safe Storage"), Firefoxa in iz datoteke.
- **M13. Spotify.** Prijava, Keychain za račune, vsi Spotify zasloni, Connect,
  vrsta, naprave, nastavitve; zvok prek libmpv.
- **M14. Sistemska integracija.** Enojna instanca, odpiranje iz Finderja in
  reopen, tipi dokumentov v `Info.plist`, element v menijski vrstici namesto
  pladnja, zapiranje v ozadje, zagon ob prijavi, sistemska obvestila.

### Del D: izdaja

- **W1. Windows na skupnem koordinatorju.** Ko je macOS vmesnik končan, se
  `win32.rs` preklopi na `apricot-shell`, da obe platformi uporabljata isto
  logiko. To enoto delava, ko je Urh pri Windows računalniku, z NVDA
  preizkusom po vsakem sklopu.

- **M15. Paket.** `.app` s pribitimi dylibi (libmpv, FFmpeg in odvisnosti),
  yt-dlp, Node, yt-dlp-ejs, ikona, ad-hoc podpis, DMG, skripta za lokalno gradnjo.
- **M16. Posodobitve.** Izbira DMG sredstva za arm64 in kanal, preverjanje
  zgoščene vrednosti, zamenjava `.app`, ponovni zagon, povrnitev; posodabljanje
  yt-dlp in pomočnika.
- **M17. Uvoz podatkov z Windows.** Uvoz mape ali ZIP-a Python ali Rust Windows
  podatkov, brez strojno vezanih poti, naprav in DPAPI poverilnic.
- **M18. CI in prevzem.** GitHub Actions za Windows in macOS (test, clippy; že
  aktivno od 11. 10. 2026),
  celoten prehod manifesta z VoiceOverjem na Urhovem MacBooku, beta DMG.

## 5. Odprta vprašanja za Urha

1. Prijava v GitHub (`gh auth login`), da lahko pushnem na vejo.
2. Ali Windows delo na `win32.rs` med M2 miruje. M2 močno preoblikuje to datoteko,
   vzporedne Windows spremembe bi povzročale težke konflikte.
3. Ali lahko po vsakem koraku M1 in M2 na Windows poženeš `cargo test` in kratek
   NVDA preizkus, ki ga napišem. Z Maca lahko Windows gradnjo samo prevedem.

## 6. Dnevnik enot

### Odločitve (10. 10. 2026)

Urh je prijavljen v GitHub. Windows delo na `win32.rs` med M2 miruje. Po vsakem
koraku M1 in M2 Urh na Windows požene `cargo test` in kratek NVDA preizkus.

### M0: gradnja celotnega delovnega prostora na macOS (10. 10. 2026)

- Odvisnost `windows` je v `apricot-platform`, `apricot-ui-windows` in obeh
  Windows spikih samo še za `cfg(windows)`. Windows koda se ni spremenila.
- `spotify_login_win32` je dobil manjkajoči `cfg(windows)`; dve Windows konstanti
  in nadomestne funkcije za druge sisteme ne sprožajo več opozoril.
- `apps/apricot-player`: posredovanje drugi instanci, izbira jezika ob prvem
  zagonu in preizkus pakiranega predvajanja so samo za Windows. Na macOS se
  program prevede, `--qualification-smoke` uspe, navaden zagon pa se ustavi pri
  manjkajočem `APPDATA`, ker macOS poti pridejo v M1.
- 13 testov, ki preverjajo Windows poti, registry in PowerShell, teče samo na
  Windows: AudioVault naslovi, pretvornik (enolične poti), piškotki (profili),
  `paths`, tri registracijski testi in dva testa updater skripte. macOS
  ustreznice pridejo v M1, M9, M12 in M16.
- Opozorilo za pozneje: na macOS shranjevanje AudioVault gesla še ne deluje
  (nadomestna funkcija vrne prazno). Keychain pride v M1 oziroma M12.
- Preverjeno: macOS `cargo build`, `cargo test --workspace` (602 uspešnih,
  0 neuspešnih, 17 izključenih), `cargo clippy --workspace --all-targets` brez
  opozoril; `cargo clippy --workspace --all-targets --target
  x86_64-pc-windows-gnu` brez opozoril; `git diff --check`.
- Na Windows naj Urh požene `cargo test --workspace` in `cargo clippy
  --workspace --all-targets`; vedenje programa se ni spremenilo, zato NVDA
  preizkus ni potreben.

### M1: platformne storitve za macOS (11. 10. 2026)

- **Predvajanje.** `libmpv`, predvajalni čas (`PlaybackRuntime`) in rezervna
  pot z zunanjim mpv se zdaj prevedejo tudi na macOS. Ime knjižnice je
  odvisno od platforme (`libmpv-2.dll`, `libmpv.2.dylib`). Rezervni mpv IPC na
  macOS uporablja Unix socket v začasni mapi; na Windows ostane named pipe z
  enakim imenom kot prej. Branje gre zdaj prek skupne `read_available`; edina
  vidna razlika na Windows je besedilo napake rezervne poti ("mpv IPC read
  failed" namesto "mpv event read failed").
- **Poti.** macOS uporablja `~/Library/Application Support/<ime>`,
  `~/Library/Caches/<ime>` in `~/Library/Logs/<ime>`, mapa za prenose je
  `~/Downloads/ApricotPlayer`. Imena map in pravilo, da beta bere stabilno
  verzijo le kot vir migracije, so enaki kot na Windows
  (`ApplicationIdentity::folder_names`).
- **Pomožni programi.** `executable_name` doda `.exe` samo na Windows; ffmpeg,
  yt-dlp, pomočnik in Node se iščejo z njim. Windows imena so nespremenjena.
- **Skrivnosti.** Na macOS je geslo AudioVault in Spotify poverilnice shranjen
  Keychain element (storitev po identiteti aplikacije); v nastavitvah ostane
  le sklic `keychain-v1:...`. DPAPI blob z Windows se na Macu ne dešifrira, kot
  je zahtevano. `forget_protected` odstrani element; UI ga začne klicati v M12
  in M13.
- **Enojna instanca.** Na macOS zaklepanje datoteke v uporabniški začasni
  mapi; test, ki je bil prej samo za Windows, zdaj teče na obeh.
- **Ostalo.** `shell::open_url` in `shell::open_path` (Explorer na Windows,
  `open` na macOS) za AppKit UI; ime Spotify naprave na macOS je ime
  računalnika iz `scutil`.
- **Preverjeno na macOS.** `cargo test --workspace`: 627 uspešnih, 0
  neuspešnih, 27 izključenih. Pravi testi z Homebrew libmpv 0.41: 8 od 9
  uspešnih; pravi IPC test z mpv prek Unix socketa uspešen; Keychain test
  uspešen. Clippy brez opozoril na macOS in za `x86_64-pc-windows-gnu`,
  `cargo fmt --check` čist.
- **Odprto za M5 in M13.** `real_libmpv_plays_a_pcm_source_across_a_seek_to_its_end`
  (pot Spotify PCM) na macOS ne uspe: po iskanju na 1,5 s ni več položajev,
  samo konec. Na Windows je test uspešen. Razlog še ni znan.
- **Na Windows ob koncu.** `cargo test --workspace`, `cargo clippy
  --workspace --all-targets`, pravi libmpv testi in kratek preizkus
  predvajanja (predvajanje, premik, naslednji posnetek).

### M2: AppKit in VoiceOver spike (11. 10. 2026)

- **Okolje za Windows teste.** Rosetta 2 je nameščena. Wine ni na voljo, ker ga
  je Homebrew 1. 9. 2026 onemogočil (ne prestane Gatekeeperja). Namesto tega
  `.github/workflows/rust-ci.yml` ob vsakem pushu na `rust-2.0` požene fmt,
  clippy (`-D warnings`) in teste na Windows in macOS GitHub runnerjih;
  repozitorij je javen, zato je brezplačno. Prvi zagon: Windows 697 uspešnih,
  macOS 627 uspešnih, 0 neuspešnih.
- **Spike** (`rust/tools/macos-accessibility-spike`, zagon s
  `rust/scripts/run_macos_spike.sh`, navodila v
  `docs/MACOS_M2_VOICEOVER_TEST.md`): AppKit prek `objc2` 0.6 in
  `objc2-app-kit` 0.3 deluje iz Rusta. Samodejno preverjeno (`--self-test`):
  izrecni vrstni red Tab in Shift+Tab gre točno skozi vse kontrole, kar zahteva
  lastno obdelavo Tab v podrazredih gumbov, pojavnega menija, drsnika in
  seznama (privzeto macOS brez "Full Keyboard Access" gumbov ne vključi v Tab);
  seznam ima ime "Main menu" in vlogo `AXTable`; drsnik ima opis vrednosti
  "50 percent".
- **Video.** libmpv na macOS ne zna risati v tuj `NSView` prek `wid` (odpre
  svoje okno). Zato ima `apricot-playback` zdaj OpenGL render API
  (`MpvVideoMode::RenderApi`, `MpvRenderHost`, modul `render.rs`): UI riše v
  `CAOpenGLLayer` na glavni niti, motor ostane na svoji niti, ob zapiranju pa
  motor počaka (največ 2 s), da UI sprosti render kontekst, preden uniči
  odjemalca. Samodejno preverjeno: približno 30 slik na sekundo v oknu
  aplikacije, brez dodatnega mpv okna. Windows pot (`Embedded`) je
  nespremenjena.
- **Opažanje.** Pravi libmpv testi na Macu niso vedno stabilni: pri zaporednih
  zagonih občasno ne uspe po en test (`starting_paused`,
  `plays_a_pcm_source_across_a_seek_to_its_end`,
  `reports_the_end_when_the_source_closes_a_waiting_generation`), naslednji
  zagon pa je uspešen. Gre za časovno občutljivost; razčistim v M5.
- **Čaka na Urha.** VoiceOver preizkus po `docs/MACOS_M2_VOICEOVER_TEST.md`
  (imena in vloge, začetnice, Enter in obnova fokusa po oknu, kontekstni meni
  s Shift+F10 in VO+Shift+M, oglasi, video na zaslonu).
- **Rezultat Urhovega VoiceOver preizkusa (11. 10. 2026).** Vseh 13 točk deluje,
  razen ponovljene začetnice: drugi P ne skoči vedno na naslednjo vrstico na P,
  ker AppKit črke v kratkem času združi v iskanje "pp". Windows seznam ob isti
  ponovljeni črki kroži med vrsticami na to črko, več različnih črk (na primer
  "pl") pa išče po začetku. To vedenje dobi lastna obdelava začetnic v
  seznamih macOS vmesnika (M3/M4), z enotskim testom.

### M3: skupni koordinator in prvo Mac okno (11. 10. 2026)

- **`apricot-shell`** (nov crate brez Win32 in AppKita): `Shell` z izbiro
  vrstice glavnega menija (`last_activated_index`, `preserved_index`, kot na
  Windows), aktivacijo vrstic (Exit konča program, ostale do svojih macOS
  zaslonov povedo `rust_feature_unavailable`), usmerjanjem tipk po pravilih
  Windows `handle_shortcut_message` (v glavnem meniju samo globalna dejanja in
  `open_selected`, ponovljena tipka brez ponovitve dejanja) in `TypeAhead`, ki
  posnema začetnice Windows seznama: ista črka kroži med vrsticami, različne
  črke iščejo po začetku, druga tipka začne novo iskanje. To popravi edino
  napako iz Urhovega M2 preizkusa. Enotski testi tečejo na obeh platformah.
- **`apricot-ui-macos`** (nov crate): aplikacija z menijsko vrstico (Settings
  s Command+vejica, Hide, Quit, Edit za besedilna polja), glavno okno 760x600
  z naslovom kot na Windows, seznam "Main menu", gumb Open in statusna
  vrstica, Tab vrstni red seznam, Open. Zapiranje okna z nastavitvijo "close
  to tray" okno skrije (klik na Dock ga vrne), sicer konča program. Izbira
  jezika ob prvem zagonu kot `first_run_language_win32.rs`. Tipke gredo prek
  okna v shell pred fokusirano kontrolo, kot na Windows prek sporočilne zanke.
- **Bližnjice na macOS.** `apricot_core::shortcut::display_shortcut` pokaže
  shranjeni `Ctrl` kot Command in `Alt` kot Option; razčlenjevalnik sprejme
  tudi "Command"/"Cmd". Tipka Control ni Apricot bližnjica (ostane sistemu).
  Shift+F10 odpre kontekstni meni.
- **Samodejni preizkus.** `APRICOT_MACOS_SELF_TEST=1` v razvojni gradnji
  pošlje oknu prave tipke in izpiše fokus, izbrano vrstico in stanje:
  P, P, P kroži (Play folder, Play file, Playlists), puščica in H skočita na
  History, Enter pove nedosegljivost, Tab gre na Open in nazaj,
  Command+Option+Y pove nedosegljivo iskanje, Escape ne naredi ničesar.
- **Za kasneje.** Besedila menijske vrstice (Hide, Quit, Edit) so angleška,
  ker jih Python nima; prevod pride z nastavitvami. Nekatere privzete
  bližnjice s Command+Option trčijo s sistemskimi (na primer Command+Option+D
  za Dock); pregled in macOS privzete vrednosti v enoti za bližnjice.
  Prikaz bližnjic v predvajalniku, iskalniku dejanj in urejevalniku bližnjic
  mora uporabiti `display_shortcut`, ko ti zasloni pridejo na Mac.
- **Preverjeno.** macOS `cargo test --workspace` 636 uspešnih, clippy čist na
  macOS in za Windows (mingw), `cargo fmt --check` čist.

### M4a: predvajalnik za lokalne datoteke (11. 10. 2026, delno)

- `apricot-shell`: zagon predvajanja, osveževanje strani predvajalnika,
  dogodki libmpv, premor in nadaljevanje, iskanje po času, čas, glasnost,
  hitrost, višina tona, ponastavitev, ponavljanje, prejšnji in naslednji,
  Escape nazaj, zadrževane tipke, kot v `win32.rs`.
- `apricot-ui-macos`: stran predvajalnika s fokusno površino "Player" (video
  prek render API), gumbi in potrditvenimi polji v vrstnem redu modela,
  Play file z odpiralnim oknom, odpiranje datotek iz Finderja.
- Samodejni preizkus (`APRICOT_MACOS_SELF_TEST=player`) s pravim mpv: video
  teče, T, Space, D, Command+0, R, puščice, V, Tab in Escape delujejo.
- Odprto: VoiceOver preizkus, Play folder, druga instanca naj preda datoteko
  prvi (zdaj se tiho konča), sistemski trki bližnjic (F11, Command+puščice).
- **Dopolnitev M4a (11. 10. 2026).** Drugi zagon preda datoteko oziroma prikaz
  okna prvi instanci prek Unix socketa ob zaklepu (`apricot_platform::
  activation`, ekvivalent Windows `WM_COPYDATA`); brez datoteke in brez
  "close to tray" drugi zagon pokaže "already open", kot na Windows. Zahteve
  med modalnim oknom počakajo. Preverjeno samodejno: drugi zagon z datoteko,
  prva instanca jo predvaja in zapiše v zgodovino. VoiceOver navodila:
  `docs/MACOS_M4A_VOICEOVER_TEST.md`. Play folder pride v M4b skupaj s
  seznamom datotek mape.

### M4b, prvi del: Play folder (11. 10. 2026)

- `apricot-shell` dobi seznamske zaslone (`ListScreen`, `ScreenControl`) s
  seznamom, gumbi v Tab vrstnem redu Windows in dejanji gumbov; glavni meni
  je zdaj tak zaslon. Nov zaslon mape (Windows `LocalFolder`): pregled mape v
  ozadju ("Loading folder..."), seznam "Play from folder" z vrsticami
  `local_file_result_line`, gumbi Play, Back, Play folder, Shuffle folder,
  Add folder to queue in Playback queue, Enter predvaja v zaporedju mape,
  Escape iz predvajalnika vrne na mapo, Escape z mape v glavni meni.
  `navigate_back` sledi Windows poti (mapa, predvajalnik, privzeto glavni
  meni brez spremembe statusa).
- Preverjeno samodejno (`APRICOT_MACOS_SELF_TEST=folder`): nalaganje, puščice,
  Enter, Escape nazaj na mapo z isto vrstico, Tab in Shift+Tab po gumbih,
  Escape v glavni meni.
- Odprto: zaslon Playback queue (gumb in Command+Option+Q zaenkrat povesta
  nedosegljivost), kontekstni meni rezultatov.

### M4b, drugi del: izhodne naprave, podrobnosti, celozaslonski način (11. 10. 2026)

- Izhodne naprave (O): izbirno okno s seznamom (`playlist_dialog_win32::choose`
  na Macu kot skupno modalno okno, ki ga uporablja tudi izbira jezika),
  `SetAudioDevice`, oglas `output_device_set`. Preverjanje shranjene naprave
  6,5 s po vidnem zagonu z vprašanjem za novo privzeto napravo, kot na Windows.
- Podrobnosti (F7): oznaka, besedilo samo za branje (puščice, Home, End,
  PageUp, PageDown, Command+C in Command+A ostanejo v besedilu), Copy details,
  Back, v Tab vrstnem redu za kontrolami; Escape najprej skrije podrobnosti,
  nato zapusti celozaslonski način, šele nato predvajalnik.
  `show_video_details_by_default` odpre podrobnosti ob odprtju predvajalnika.
- Celozaslonski način (F11 in potrditveno polje): nativni macOS full screen
  okna, kadar je odprta stran predvajalnika v tem načinu.
- Popravek: gumb "Back to main menu" v predvajalniku zdaj ustavi predvajanje
  in gre v glavni meni (Windows `leave_player_to_main_menu(false)`).
- Preverjeno samodejno s pravim mpv: F7, puščica v podrobnostih, Escape skrije,
  F11, Escape zapusti celozaslonski način, Escape v glavni meni.

### M4c: izenačevalnik predvajalnika F4 (11. 10. 2026)

- Logika okna `equalizer_win32.rs` je zdaj v `apricot-shell`
  (`equalizer_dialog.rs`, vmesnik `EqualizerView`), tako da jo bo v W1
  uporabil tudi Windows. Vsebina in vedenje sta kot na Windows: prednastavitev,
  razpon v dB, ime lastne prednastavitve (vidno samo za lasten profil, takrat
  je omogočen tudi Delete profile), drsnik za vsak pas z imenom pasu in
  vrednostjo v dB, enajst gumbov v vrstnem redu Windows, sprotna uporaba po
  160 ms, Compare, profili (dodaj, izbriši z Da/Ne, uvoz in izvoz JSON),
  privzeto za napravo. OK shrani ime in razpon, Cancel, Escape in zaprtje
  vrnejo prejšnji izenačevalnik; oglasi `equalizer_saved` ali
  `equalizer_closed`.
- macOS okno: `PopUp` se obnaša kot spustni seznam Windows (puščice, Home,
  End in črke takoj zamenjajo izbiro, preslednica odpre seznam), `Slider` kot
  trackbar (korak 1 dB, Page Up in Page Down 3 dB, Home in End do roba,
  VoiceOverjevo povečanje in zmanjšanje po 1 dB). Odobrena prilagoditev za
  Mac: puščica gor in desno povečata vrednost, kot pri drsnikih macOS.
  Return pritisne OK, Escape Cancel, kot `IDOK` in `IDCANCEL`. Onemogočeni in
  skriti gumbi niso več v Tab vrstnem redu (velja za vse gumbe).
- Novo skupno okno za vnos imena (`prompt_text`, Windows `prompt_name`) in
  besedili `yes` ter `no` za vprašanja (Windows uporabi sistemska gumba).
- Časovnika predvajanja in dela v ozadju tečeta tudi med modalnimi okni,
  kot na Windows.
- Preverjeno: enotski testi okna v `apricot-shell` ter samodejni preizkus s
  pravim mpv (`APRICOT_MACOS_SELF_TEST=player`): F4, puščice v
  prednastavitvi, Tab na razpon in prvi pas, gor in Page Up, Tab in
  Shift+Tab, Escape, fokus nazaj na predvajalnik in oglas "Equalizer closed".

### M4d: poglavja, BPM, ReplayGain in kopiranje povezav (11. 10. 2026)

- `apricot-shell/src/player_tools.rs`: Windows `cycle_player_replaygain`,
  `copy_active_location` (L), `copy_current_timestamp_link`,
  `copy_active_stream_url`, `seek_relative_player_chapter` (Option+levo in
  Option+desno), `show_player_chapters` (seznam poglavij s trenutnim
  poglavjem izbranim, Play in Back), prenos poglavij iz `chapters_url` v
  ozadju in analiza BPM (B) s ffmpeg v ozadju; rezultat, ki ne velja več
  (drug posnetek, hitrost ali višina), se zavrže kot na Windows.
- Odprto: razreševanje toka za YouTube in SoundCloud pri Copy stream URL
  pride z iskanjem (M6). Seznam poglavij sprejme Enter in Escape; po meri
  nastavljeni bližnjici za `open_selected` in `player_back` (Windows
  `PickerBehavior`) pridejo z urejevalnikom bližnjic (M10). Paket za Mac
  (M15) mora vključiti ffmpeg, ker zagon iz Finderja nima Homebrew poti.
- Preverjeno samodejno (`APRICOT_MACOS_SELF_TEST=tools` z datoteko s tremi
  poglavji in ritmom 120 BPM): naslednje in prejšnje poglavje z oglasom,
  ReplayGain, "120 BPM.", kopiranje poti, sporočili za časovno povezavo in
  tok lokalne datoteke, seznam poglavij (fokus na seznamu, Down, Return
  skoči na poglavje, fokus nazaj na predvajalnik).

### M4e: način urejanja, oznake izseka in zaznamki (11. 10. 2026)

- `player_edit.rs`: način urejanja (E), shranjevanje kopije (Command+S) in
  zamenjava izvirnika (Command+R) v ozadju z mpv in ffmpeg kot na Windows,
  oznaki začetka in konca izseka ([ in ]) ter predogled izseka (P), ki se
  ustavi ob premiku, pavzi ali novem posnetku.
- `bookmarks.rs` v jedru in okno zaznamkov na Macu: dodajanje s privzetim
  imenom po času (Command+Shift+B), okno zaznamkov iz predvajalnika
  (Command+Shift+K, samo trenutni posnetek) in iz glavnega menija (vsi),
  seznam s praznim besedilom, gumbi Add, Play, Rename, Delete, Copy
  timestamp link in Back v vrstnem redu Windows, Enter predvaja, Delete
  izbriše, Escape zapre, Shift+F10 odpre kontekstni meni z enakimi
  ukazi. Play zaznamka drugega posnetka odpre predvajalnik na poziciji,
  Escape iz njega vrne v okno zaznamkov.
- Novo v `ShellUi`: `prompt_text` (Windows `prompt_name_with_initial`).
  Return v polju za ime zdaj pritisne OK tudi v izenačevalniku.
- Odprto za paket (M15): Mac paket mora vključiti ukaz mpv poleg libmpv,
  sicer shranjevanje urejanja uporabi samo ffmpeg (tako kot Windows brez
  mpv.exe).
- Preverjeno samodejno (`APRICOT_MACOS_SELF_TEST=tools`): oznake in
  sporočila o neveljavnem izseku, način urejanja, shranjena kopija
  "beats - edited.m4a", dodajanje zaznamka z Return, preklic z Escape,
  okno zaznamkov (fokus na seznamu, Tab na Add in Play, Escape, fokus nazaj
  na predvajalnik).

### M4f: besedila pesmi (11. 10. 2026)

- `apricot-shell/src/lyrics.rs` in okno besedil na Macu (Windows
  `show_player_lyrics` z `details_win32::show_async`): besedilo samo za
  branje s fokusom, Copy lyrics, Back in statusna vrstica; najprej
  "Fetching lyrics", nato lokalna datoteka (.lrc, .txt) ali spletni vir
  lrclib, oglas naslova ali "No lyrics available", sprotno označena časovna
  vrstica (samo vizualno, kot na Windows). Escape in zaprtje vrneta fokus v
  predvajalnik. Kopirano besedilo ima na Macu vrstice LF.
- Prestavljeno: prepis (Command+Shift+T) in komentarji (Command+Shift+M)
  potrebujeta YouTube in yt-dlp, zato prideta z iskanjem (M6); Spotifyjeva
  besedila in prepisi epizod pridejo s Spotifyjem (M13).
- Preverjeno: enotski testi nalaganja, kopiranja in obsegov časovnih vrstic;
  samodejni preizkus s pravim predvajanjem in lokalno .lrc datoteko
  (fokus na besedilu, Tab na Copy lyrics, Return kopira, Escape zapre,
  fokus nazaj na predvajalnik).
