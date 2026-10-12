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

- **S1. Spotify: Followed artists (nova funkcija, Urhova zahteva 12. 10. 2026).**
  Po koncu macOS porta, za Windows in macOS hkrati: nova vrstica "Followed
  artists" v Spotify meniju takoj pod Playlists (`SpotifyHubEntry` v
  `apricot-app/src/spotify.rs`), seznam vseh izvajalcev, ki jim račun sledi
  (Spotify Web API `GET /me/following?type=artist` s straničenjem prek
  `after`, potreben obseg `user-follow-read`), Enter odpre izvajalca kot
  drugje. Besedila v vseh jezikih, test in kratek preizkus z NVDA in
  VoiceOverjem. To je zavestna razlika od Python verzije.

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

## 4a. Napake in želje za odpravo pred izdajo beta 6

Beta 6 (zadnja izdana je beta 5) bo izšla za Windows in macOS z macOS
podporo in temi popravki skupaj (Urh, 12. 10. 2026).

1. **Spotify po premoru izgubi prijavo (Windows, 12. 10. 2026).** Urh je
   predvajal prek Spotifyja, dal na pavzo in šel za nekaj minut stran. Ob
   vrnitvi je bilo prikazano sporočilo v smislu "pmc server not found",
   nato je predvajanje javilo "Spotify not logged in". Po ponovnem zagonu
   programa je vse delovalo. Najprej ugotovi, ali gre za napako (na primer
   izgubljena povezava ali žeton seje Spotify, ki se ne obnovi), nato jo
   popravi za obe platformi.
   Stanje (12. 10. 2026): verjeten vzrok je, da se seja librespot med
   mirovanjem prekine (omrežje, spanje), program pa je pove samo kot
   "disconnected" in se ne poveže znova. Zdaj se na obeh platformah po
   izgubljeni seji sam znova poveže z aktivnim računom (največ enkrat na
   minuto). S pravim računom še ni preizkušeno.
2. Kolekcija "More" na Windows dvakrat doda pot (glej M6, popravi v W1).
3. **Liked songs brez smart shuffle.** Popravek Urha (12. 10. 2026):
   smart shuffle deluje, ko Liked songs odpre prek Library, ne pa prek
   vrstice Liked songs v Spotify meniju (tam samo shuffle vklop in izklop).
   Vrstica v meniju naj odpre natanko isti seznam kot Liked songs v
   Library (isti vir, isti kontekst), ne posebnega Apricotovega seznama.
   Obe platformi.
   Stanje: v kodi obeh platform vrstica v meniju in vrstica v Library že
   odpreta isti vir (`Source::LikedSongs`) z istim kontekstom
   (`spotify:user:<id>:collection`), smart shuffle pa je dovoljen samo za
   `spotify:playlist:`. Razlike brez pravega računa ne morem ponoviti;
   preveri z Urhom (kateri kontekst Spotify sporoči v obeh primerih).
4. **Pitch in hitrost se ohranita med predmeti.** Kot glasnost in bass
   boost naj se pitch in hitrost ohranita, ko z Escape zapustim predvajalnik
   in predvajam nekaj drugega; na privzeto se vrneta samo ob izrecnem
   zaprtju predvajalnika (Close). Pri prev/next se pitch že ohrani. Enako
   naj se ohrani način shuffle (tudi Spotify shuffle in smart shuffle), ko
   je vklopljeno predvajanje v ozadju in predvajalnik ni zaprt z gumbom
   Close. Obe platformi.
   Stanje (12. 10. 2026): pitch in hitrost sta narejena v skupnem
   `PlayerSession` (obe platformi): ostaneta, dokler je predvajalnik odprt,
   Close ju vrne na nastavljene vrednosti. Ohranjanje Spotify shuffle in
   smart shuffle čaka na preizkus s pravim računom, ker je odvisno od
   Spotify Connect.
5. **Izbira več skladb v Spotifyju za dodajanje na seznam.** Predlagaj
   sistem pred implementacijo (na primer Ctrl+A izbere vse rezultate, nato
   izbira seznama) in opozorilo, ko je skladba že na seznamu, da ne nastanejo
   dvojniki. Preveri, ali Shift+A za posamezne skladbe deluje, kot je prej.
   Obe platformi, na koncu rewrita.
   Narejeno (12. 10. 2026, Urhova odločitev): v Spotify seznamu Shift+A
   (dejanje `queue_audio`, uporabnik ga lahko prestavi) označi ali odznači
   skladbo in pove število označenih; Spotify skladbe ne gredo več v vrsto
   prenosov. Ctrl+A (na Macu Command+A) najprej naloži preostale strani in
   označi vse skladbe celotnega seznama. Označene vrstice imajo na koncu
   "marked". Add to Spotify playlist doda označene (sicer izbrano vrstico);
   za obstoječi seznam ga prej v celoti prebere in, če so nekatere skladbe
   že tam, vpraša: dodaj samo nove ali dodaj vse. Po potrditvi Spotifyja se
   oznake počistijo. Obe platformi; enotni test na Macu, s pravim računom
   ni preizkušeno.
6. **Posodabljanje librespot.** librespot je v programu preveden kot
   knjižnica, zato se ga ne da posodobiti kot yt-dlp; nova verzija pride z
   novo izdajo programa. Pred objavo preveri, ali je potrebna novejša
   verzija (0.8.0 je pripeta), in o tem obvesti Urha.
   Stanje (12. 10. 2026): na crates.io je 0.8.0 še vedno najnovejša
   različica, posodobitev ni potrebna.
7. **Fokus po vrnitvi v okno.** Ko z Alt+Tab ali Shift+Tab preklopim v drugo
   okno in nazaj, bralnik pove samo ime okna; fokus naj se vrne na zadnjo
   kontrolo. Obe platformi.
   Stanje: Windows zdaj ob vrnitvi iz drugega programa (`WM_ACTIVATE`, ko
   je bilo prej aktivno okno drugega procesa) da fokus kontroli, ki ga je
   imela ob odhodu, sicer glavni kontroli zaslona; ob zaprtju lastnih
   pogovornih oken ostane kot prej. Mac to že dela (okno obdrži fokus,
   preverjeno s samodejnim preizkusom skrivanja in prikaza). Na Windows z
   NVDA še ni preizkušeno.
8. **Enter v iskalnem polju Spotify** na Windows še enkrat pove
   ApricotPlayer in list, preden so rezultati. Popravi, če je vzrok jasen,
   sicer ne posegaj.
   Stanje: vzrok ni povsem jasen. Iskanje Spotify je edino iskanje z
   ločenim pogovornim oknom; ko se zapre, Windows vrne aktivnost glavnemu
   oknu (NVDA pove ime okna), nato dobi fokus seznam, ki ima do prihoda
   rezultatov samo vrstico "Loading". Na Macu seznam dobi fokus brez
   vmesne kontrole. Na Windows nisem posegal; za popravek bi bilo treba
   iskanje Spotify spremeniti v iskalni zaslon brez pogovornega okna, kot
   je iskanje YouTube. Odloči Urh.

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

### M5a: Playback queue in kontekstni meniji (11. 10. 2026)

- `apricot-shell/src/queue.rs`: okno Playback queue (Windows
  `show_playback_queue` s `playback_queue_win32.rs`): navodila, seznam s
  številko, kanalom in vrsto, Play, Move up, Move down, Remove, Clear in
  Back v vrstnem redu Windows, Enter predvaja, Escape zapre, kontekstni
  meni z enakimi ukazi, okno se zapre, ko je vrsta prazna; spremenjena
  vrsta se shrani, izbrani posnetek se predvaja in odstrani iz vrste
  (`QueueStartMode::Matching`). Dodajanje in odstranjevanje aktivnega
  posnetka (Command+Shift+Q, Command+Shift+Delete) z oglasi Windows.
- Kontekstni meniji seznamov in predvajalnika iz `apricot_app::context_menu`
  (Shift+F10, VoiceOver ukaz za meni, desni klik na seznamu); bližnjice v
  oznakah menijev so na Macu prikazane s Command in Option. Ukazi, ki
  pridejo s poznejšimi enotami (prenosi, priljubljeni, seznami predvajanja,
  kanali), povedo, da še niso na voljo.
- Odprto: predvajanje v ozadju z vrstico predvajalnika na drugih zaslonih
  (Windows `background_player`, nastavitev `enable_background_playback`,
  privzeto izklopljena) pride v posebni enoti; do takrat se izbrani
  posnetek iz vrste vedno odpre v predvajalniku.
- Preverjeno samodejno (`APRICOT_MACOS_SELF_TEST=folder`): dodajanje v
  vrsto, sporočilo "already in the playback queue", kontekstni meni z
  vsebino in izvedbo "Remove from playback queue", okno vrste (fokus na
  seznamu, Down, Tab na Play in Move up, Return premakne, Escape zapre).

### M6a: iskalni zaslon, rezultati in predvajanje prek yt-dlp (11. 10. 2026)

- `apricot-shell/src/search.rs` in `youtube.rs`: iskalni zaslon (Windows
  `show_search`) s poljem za iskanje, ponudnikom (YouTube, SoundCloud) in
  vrsto, gumbi Search, Play, Download audio, Download video in Add to
  favorites ter seznamom rezultatov v vrstnem redu Tab kot na Windows
  (Back, iskanje, ponudnik, vrsta, gumbi, seznam). Return v polju išče,
  Escape gre nazaj, na polju in gumbih navadne tipke ostanejo kontroli,
  Command in Option bližnjice delujejo na izbranem rezultatu
  (Python `on_char_hook`). Rezultati z vrsticami Windows `result_label`,
  "Found results", nalaganje več rezultatov na zadnji vrstici, napake z
  oknom in fokusom nazaj na polje.
- Predvajanje rezultata: razreševanje toka prek yt-dlp v ozadju s
  predpomnilnikom tokov, stran predvajalnika takoj ("Preparing playback"),
  nato predvajanje; Copy stream URL za YouTube in SoundCloud; prijazna
  sporočila o napakah. Escape iz predvajalnika vrne na rezultate.
- macOS: polja zaslona (`screen_fields.rs`) z oznako nad kontrolo, seznam
  sporoča spremembo izbire. Razvojne različice uporabijo Homebrew yt-dlp,
  paket (M15) mora vključiti yt-dlp v mapo components.
- Odprto za M6b: dopolnjevanje metapodatkov rezultatov, kanali in seznami
  predvajanja (zdaj povedo, da še niso na voljo), možnosti kanala, Trending,
  Direct link, prepis, komentarji, sorodni videi, vnaprejšnje razreševanje
  naslednjega toka, vprašanje za osvežitev piškotkov (M12).
- Preverjeno samodejno s pravim yt-dlp (`APRICOT_MACOS_SELF_TEST=search`):
  Command+Option+Y odpre iskanje s fokusom v polju, tipkanje, Return,
  "Found results: 20" s fokusom na seznamu, Down in Return predvaja
  (video sličice tečejo), Escape nazaj na rezultate z isto vrstico,
  Shift+Tab na Add to favorites, Escape v glavni meni.

### M6b: kanali in seznami predvajanja (11. 10. 2026)

- `apricot-shell/src/collections.rs`: vsebina kanala ali seznama
  predvajanja na iskalnem zaslonu (Windows `open_youtube_collection`,
  `refresh_youtube_collection`, nalaganje več na zadnji vrstici), možnosti
  kanala (videi, seznami predvajanja, prenosi v živo, priljubljeni), izvajalci
  SoundClouda odprejo svoje skladbe, Open channel, Play playlist in Shuffle
  playlist iz kontekstnega menija. Escape iz zbirke vrne na rezultate, napaka
  pri prvem nalaganju tudi. Vsi začetki predvajanja zdaj gredo skozi Windows
  `start_media_item_with_options`, zato tudi naslednji in prejšnji posnetek
  na seznamu YouTube razrešita tok.
- Odstopanje od Windows (napaka v Windows, popraviti v W1): Windows ob
  nalaganju več vrstic zbirke na sklad poti doda isto pot še enkrat, zato
  bi Escape potreboval dva pritiska; Mac jo doda samo ob prvem nalaganju.
- Preverjeno samodejno s pravim yt-dlp (`APRICOT_MACOS_SELF_TEST=search`,
  `APRICOT_MACOS_SELF_TEST_KIND=playlist`): Tab na ponudnika in vrsto,
  puščice izberejo Playlist, gumb Search, Return odpre seznam predvajanja,
  Down in Return predvajata, Escape nazaj na seznam, Escape na rezultate,
  Escape v glavni meni.

### M6c: Trending in Direct link (11. 10. 2026)

- `apricot-shell/src/trending.rs`: zaslon Trending (Windows `show_trending`)
  z državo in kategorijo, seznamom, Back in Load trending v vrstnem redu
  Tab Windows; sprememba države ali kategorije takoj naloži lestvico;
  uradni YouTube Data API s ključem, sicer javna lestvica prek yt-dlp,
  napaka z oknom in vrnitvijo v glavni meni; Back iz predvajalnika obnovi
  izbiro države in kategorije. Zaslon Direct link (Windows
  `show_direct_link`) s poljem za povezavo, Back, Play, Download audio,
  Download video in Copy stream URL, Return izvede nastavljeno dejanje
  (`direct_link_enter_action`), povezava brez sheme dobi `https://`,
  yt-dlp se zažene vnaprej med tipkanjem. Zaslon brez seznama skrije
  seznam.
- Ugotovitev (velja tudi za Windows): brez ključa YouTube Data API javne
  lestvice za kategorijo All ni; za Music YouTube stran
  `charts.youtube.com/charts/TrendingVideos` zdaj vrne 404, zato Trending v
  praksi deluje samo s ključem API. Predlog za popravek v obeh verzijah.
- Prenosi in Spotifyjeve povezave na zaslonu Direct link pridejo s svojima
  enotama (M9, M13).
- Preverjeno samodejno (`APRICOT_MACOS_SELF_TEST=trending`): Trending brez
  ključa pokaže sporočilo, Return ga zapre, glavni meni z vrstico Trending;
  Command+Option+L odpre Direct link s fokusom v polju, Return predvaja
  povezavo prek yt-dlp, Escape vrne na zaslon z ohranjeno povezavo, Tab na
  Back, Escape v glavni meni; nedosegljiv video pokaže napako yt-dlp.

### M6d: prepis, komentarji in sorodni videi (11. 10. 2026)

- Prepis (Command+Shift+T, Windows `show_player_transcript` s
  `transcript_win32.rs`): iskalno polje s fokusom, seznam vrstic, Play,
  Copy line, Copy transcript, Copy timestamp link in Back; Enter v polju ali
  na seznamu skoči na vrstico, tipkanje filtrira, prepis se naloži v ozadju
  (lokalni podnapisi ali YouTube prek yt-dlp) in se shrani za sejo, omejitev
  YouTube (429) se pokaže kot pri Windows. `transcript_loader.rs` je kopija
  Windows datoteke in se v W1 preseli v jedro.
- Komentarji (Command+Shift+M, Windows `show_player_comments` s
  `comments_win32.rs`): iskanje, razvrščanje, seznam, Open (okno s
  podrobnostmi), Copy, Copy visible, Author channel, Load more in Back,
  kontekstni meni, strani prek YouTube Data API ali yt-dlp.
- Sorodni video (Command+Shift+Page Down) in samodejni sorodni video na
  koncu posnetka z nastavitvijo `autoplay_related`.
- Preverjeno samodejno s pravim yt-dlp (`APRICOT_MACOS_SELF_TEST=trending`,
  `APRICOT_MACOS_SELF_TEST_KIND=extras`): prepis se odpre s fokusom v polju
  in pokaže sporočilo YouTube o omejitvi, komentarji se naložijo (fokus v
  polju, Tab na razvrščanje in seznam, vrstice s komentarji, Tab na Open,
  Escape), sorodni video začne naslednji posnetek.

### M7a: priljubljeni in zgodovina (11. 10. 2026)

- `apricot-shell/src/library.rs`: zaslona Favorites in History (Windows
  `show_media_collection`, `refresh_media_collection`) s seznamom, Play,
  Back, Remove in pri zgodovini Clear history v vrstnem redu Windows; vrstice
  zgodovine s časom in dejanjem; Enter predvaja (kanal in seznam
  predvajanja odpreta svoje videe); Command+F doda med priljubljene,
  Command+Shift+F odstrani, Delete odstrani izbrano vrstico; kontekstna
  menija priljubljenih in zgodovine; Back iz predvajalnika vrne na zaslon.
  Gumb Add to favorites na iskalnem zaslonu zdaj deluje.
- Tipka Delete na Macu (nad Return) deluje kot Windows Delete v seznamih;
  v besedilnih poljih še vedno briše znake.
- Preverjeno: enotska testa (vrstni red kontrol, dodajanje, Delete,
  počisti zgodovino) in samodejni preizkus na Macu (Command+F, sporočilo
  "already in favorites", Command+Option+F odpre priljubljene, Tab na Play,
  Shift+Tab na seznam, Delete odstrani, Escape v glavni meni na vrstico
  Favorites).

### M7b: seznami predvajanja (11. 10. 2026)

- `apricot-shell/src/playlists.rs`: zaslona Playlists in Playlist items
  (Windows `show_user_playlists`, `show_user_playlist_items`) z vrstnim
  redom Tab Windows (Back, Create playlist, Open playlist, Download
  playlist, Remove playlist, seznam; vsebina: Back, Play, Download
  playlist, Remove from playlist, seznam, pri praznem seznamu samo Back in
  seznam); ustvarjanje z imenom (Command+Shift+N), dodajanje aktivnega
  posnetka (Command+P: nov seznam, edini seznam ali izbira), odstranjevanje
  (Command+Shift+P, Delete), podmeni Add to playlist v kontekstnem meniju,
  predvajanje v zaporedju seznama z označitvijo trenutne vrstice.
- Naslednji in prejšnji posnetek na koncu naloženih rezultatov iskanja ali
  zbirke zdaj počakata na naslednjo stran in nadaljujeta (Windows
  `pending_player_navigation`).
- Enter na praznem iskalnem zaslonu pove "no selection" kot na Windows.
- Prenos seznama pride s prenosi (M9).
- Preverjeno: enotski test (vrstni red kontrol, ustvarjanje, odpiranje,
  odstranjevanje) in samodejni preizkus na Macu (Command+P ustvari seznam
  Mix z vnosom imena, drugi Command+P, Command+Option+P odpre sezname,
  Return odpre seznam, Delete odstrani posnetek, Shift+Tab na Back, Escape
  na sezname, Delete odstrani seznam).

### M8: podcasti in RSS (11. 10. 2026)

- `apricot-shell/src/podcasts.rs` in `podcast_work.rs` (zaenkrat kopija
  Windows `podcast_win32.rs`, podvojitev odpravi W1): zasloni Podcasts and
  RSS feeds, Feed items, Podcast search results in Podcast categories z
  vrstnim redom Tab Windows; dodajanje feeda (povezava brez sheme dobi
  https://), iskanje v imeniku, kategorije, osvežitev enega ali vseh feedov,
  odstranjevanje (gumb, Delete, kontekstni meni), filter in nastavitev
  kategorije, prednastavljena hitrost, označitev predvajanega
  (Command+Shift+X), brisanje napredka (Command+Shift+R), shranjevanje
  hitrosti (Command+Shift+E), uvoz in izvoz OPML (okni za odpiranje in
  shranjevanje, privzeto ime apricot_feeds.opml), odpiranje v brskalniku in
  kopiranje URL feeda.
- Epizoda se označi kot predvajana ob koncu in ob zaprtju blizu konca kot
  na Windows; nove epizode gredo v center obvestil (sistemsko obvestilo
  pride z M14).
- Časovnik samodejne osvežitve (0,5 do 168 ur) in osvežitev ob zagonu.
- End ali Down na zadnji vrstici naloži naslednji paket epizod.
- Splošen popravek seznamov na Macu: Home, End, Page Up in Page Down zdaj
  premaknejo izbiro kot v seznamu Windows (NSTableView je samo drsel).
- Prenosi epizod in feedov pridejo z M9.
- Preverjeno: enotski testi (vrstni red kontrol, odpiranje, označitev,
  nazaj, časovnik) in samodejni preizkus na Macu z resničnim feedom Planet
  Money (dodajanje, 100 od 355 epizod, označitev in odznačitev, predvajanje
  epizode z mpv, End naloži še 100 in izbere zadnjo vrstico, Escape nazaj).

### M9a: prenosi (12. 10. 2026)

- `apricot-shell/src/downloads.rs` in `download_work.rs` (kopija Windows
  `download_win32.rs` do W1): zaslon Current downloads z vrstnim redom Tab
  Windows (Back, pri čakajočih Download selected, Download all as audio in
  video, pri aktivnih Cancel in Cancel all, seznam), oznake vrstic,
  kontekstni meni po vrsti, Command+Shift+A in Command+Shift+D (z zaščito
  pred dvojnim pritiskom 0,35 s), Shift+A doda ali odstrani iz vrste,
  izbira zvok ali video pri izbranih, potrditev pred prenosom (Da/Ne),
  izbira mape ali datoteke, kadar je vklopljeno spraševanje, mape po
  Python razporedu (music, podcasts, ime feeda ali zbirke), ponovni
  poskus s piškotki, prenos vseh čakajočih, prenos seznama predvajanja in
  feeda, preklic enega ali vseh, zgodovina prenosov, sporočilo ob koncu
  in odpiranje mape.
- Okno napredka (`apricot-ui-macos/src/download_progress.rs`): sporočilo,
  vrstica napredka, Hide (privzeti gumb s fokusom) in Details, ki odpre
  zaslon prenosov; zapiranje ga samo skrije, kot na Windows.
- Gumbi za prenos na iskalnem zaslonu, v neposredni povezavi, pri seznamih
  in podcastih so zdaj povezani; oznaka vrste na vrstici rezultata se
  osveži, ko se izbira premakne, kot na Windows.
- Delete na seznamu feedov ne odstrani več feeda (na Windows tega ni bilo).
- Izvoz izseka iz predvajalnika pride z M9b skupaj s pretvornikom;
  sistemsko obvestilo ob koncu prenosa v ozadju pride z M14.
- Preverjeno: enotski testi (zaslon in vrsta prenosov, zavrnjena potrditev,
  mape, napredek, zaščita bližnjice) in samodejna preizkusa na Macu z
  resničnim yt-dlp: epizoda Planet Money dodana v vrsto s Shift+A, odprt
  zaslon prenosov, Return prenese datoteko v mapo podcasts/Planet Money;
  seznam Mix s to epizodo prenesen z gumbom Download playlist v
  music/Mix, okno napredka se je odprlo, posodabljalo in zaprlo. Fokusa v
  oknu napredka med preizkusom ni bilo mogoče preveriti, ker aplikacija
  ni bila v ospredju.

### M9b: pretvornik in izvoz izseka (12. 10. 2026)

- `apricot-shell/src/converter.rs`: odločitve pretvornika datotek in map
  (Windows `converter_win32.rs`: posodabljanje formatov ob spremembi poti z
  ohranitvijo izbire, prikaz možnosti zvok v video, pari potrditvenih polj,
  preverjanja in sporočila), zagon pretvorbe, napredek pretvorbe mape,
  sporočila ob koncu (`popup_when_conversion_complete`) in izvoz označenega
  izseka iz predvajalnika (Command+Shift+A ali D z oznakama) v mapo clips
  z enakim poimenovanjem kot na Windows.
- `apricot-ui-macos/src/converter.rs`: modalno okno pretvornika z vrstnim
  redom Tab Windows (pot, Browse, format, Add image, Dark background, pot
  slike, Choose image, Create new, Replace, Convert, Back), skrite vrstice
  se umaknejo iz postavitve, Escape je Back, opozorila vrnejo fokus na
  kontrolo; novo potrditveno polje `CheckBox` (Space preklopi). Okno
  napredka pretvorbe mape brez gumbov, ki se ne da zapreti, s pretečenim,
  ocenjenim in preostalim časom.
- Razlika: macOS okna za izbiro datoteke nimajo seznama filtrov, zato
  dovolijo samo prvi filter (predstavnostne datoteke ali slike); "All
  files" ni na voljo. Brez najdenega FFmpeg izvoz izseka takoj javi napako.
- Preverjeno: enotski testi (formati, opozorilo brez datoteke, preklicano
  shranjevanje, zamenjava izvirnika) in samodejni preizkus na Macu s
  pravim ffmpeg: pretvorba datoteke z Replace, pretvorba mape z dvema
  datotekama, izvoz izseka 0:00 do 0:02 iz predvajalnika (drugič z
  "(2)"); fokus se po zaprtju pretvornika vrne na glavni meni. Okna
  napredka pretvorbe mape preizkus ni ujel, ker je bila pretvorba
  prehitra.

### M10: nastavitve, Action Finder in bližnjice (12. 10. 2026)

- `apricot-shell/src/settings_window.rs`: odločitve okna nastavitev
  (Windows `settings_win32.rs`): vezave kontrol na nastavitve, uporaba
  vidnih vrednosti, Save (z jezikovno spremembo okno zgradi znova), Restore
  to defaults, ponastavitev razdelka, izbira mape za prenose, izenačevalnik
  (predogled drsnikov, ponastavitev, profili prek istih pozivov kot okno
  F4), naprave za predvajanje (sondiranje v ozadju in zamenjava izbire na
  mestu), zajem bližnjic z opozorilom ob spopadu in osvežitev glavnega
  okna po zaprtju z odloženim globalnim dejanjem.
- `apricot-ui-macos/src/settings.rs`: modalno okno z vrstnim redom Tab
  Windows (Back, Save, Restore to defaults, seznam razdelkov, kontrole
  razdelka), Return shrani, Escape je Back, puščice v seznamu razdelkov
  pokažejo razdelek po 140 ms, Tab ali Return ga pokaže takoj in premakne
  fokus vanj, Shift+Tab s prve kontrole nazaj na seznam, puščice med
  potrditvenimi polji menija, polje za zajem bližnjic zajame vse tipke
  razen Tab (tudi Command), globalne bližnjice zaprejo okno in se izvedejo
  v glavnem oknu, Action Finder se odpre nad nastavitvami.
- `apricot-ui-macos/src/action_finder.rs`: Action Finder z iskalnim
  poljem, seznamom, Open in Cancel; puščici iz polja v seznam, Return
  izvede, Escape zapre. Oznake bližnjic v Action Finderju so na Macu
  Command in Option (na Windows nespremenjeno).
- Na Macu pridejo kasneje (sporočilo, da funkcija še ni na voljo):
  piškotki in AudioVault (M12), privzeti predvajalnik in naročnine (M14),
  preverjanje posodobitev (M16).
- Opomba: na začetku modalne seje AppKit premakne fokus na Back, zato ga
  okno po 0,3 s vrne na seznam razdelkov, če ga uporabnik še ni premaknil.
- Preverjeno: enotski testi (vezava potrditvenega polja, spopad in zajem
  bližnjice, zapis bližnjic, odvisni razdelki, Action Finder) in
  samodejni preizkus na Macu: Action Finder ("favor", puščica dol, Return
  odpre priljubljene), nastavitve (fokus na seznamu razdelkov, puščica dol
  na Customize main menu, Tab v razdelek, Shift+Tab nazaj, End na
  Keyboard shortcuts, izbira dejanja, zajem Command+Shift+J javi, da je
  že dodeljena Action finderju, Escape zapre okno, fokus se vrne na seznam
  priljubljenih).

### M12a: piškotki (12. 10. 2026)

- `apricot-platform/src/browser_cookies.rs`: na Macu mape profilov
  brskalnikov v `~/Library/Application Support` (Chrome, Brave, Edge,
  Chromium, Vivaldi, Opera, Firefox), programi v `/Applications`, ali brskalnik
  teče (`pgrep`) in zapiranje (`osascript ... quit`). Windows poti
  nespremenjene.
- `apricot-shell/src/cookies.rs` (kopija nevtralnih delov Windows
  `cookies_win32.rs`): izvoz piškotkov iz brskalnika prek yt-dlp ali
  `DevTools`, samodejna obnova ob napaki prijave v delavcih yt-dlp s
  petminutnim premorom po neuspehu, ponovni poskus predvajanja po vprašanju
  "osveži piškotke" (Da/Ne), uvoz datoteke s piškotki, odpiranje prijave v
  YouTube v profilu brskalnika.
- Nastavitve: Choose cookies file, Open YouTube login profile in Export
  browser cookies (z vprašanjem za zaprtje odprtega brskalnika) zdaj
  delujejo; polje in izbira brskalnika se po izvozu posodobita tudi, če je
  okno nastavitev odprto.
- Odprto vprašanje: Safari ni na seznamu brskalnikov (enako kot na
  Windows). yt-dlp ga podpira (`--cookies-from-browser safari`, potreben je
  Full Disk Access); dodati ga je mogoče samo za Mac, če Urh želi.
- Preverjeno: enotski test uvoza datoteke s prijavo, testi platforme.
  Pravega izvoza iz brskalnika na tem Macu nisem poganjal, ker bi bral
  piškotke iz Urhovih brskalnikov in sprožil vprašanje Keychaina.

### M12b: AudioVault (12. 10. 2026)

- `apricot-shell/src/audiovault.rs` (Windows `audiovault_win32.rs`):
  prijava (shranjeno geslo v Keychainu prek obstoječega
  `audiovault_credentials`, sicer okno za prijavo), meni (Search, Recent TV
  shows, Recent movies), iskanje (Back, iskalno polje, vrsta, Search, Play,
  Download audio, seznam; Return v polju išče), nedavni naslovi, epizode
  oddaj (manifest arhiva, celoten prenos, kadar strežnik ne podpira
  razponov), predvajanje filmov in epizod (s predpomnilnikom in njegovim
  obrezovanjem), prenosi filmov, epizod in celih oddaj, ponovna prijava ob
  potekli seji, vrnitev iz predvajalnika na isti zaslon, kontekstni meni
  (Open, Download), nastavitve Log in, Log out in Register.
- `apricot-ui-macos/src/audiovault_login.rs`: okno za prijavo (Email s
  fokusom, Password, Register, OK, Cancel; Return v polju je OK).
- Razlika: naslov nad seznamom rezultatov (statično besedilo na Windows) ni
  prikazan posebej; seznam ima naslov kot ime.
- Preverjeno: enotski test (brez prijave se pokaže okno za prijavo in
  zaslon ostane na glavnem meniju), samodejni preizkus okna za prijavo na
  Macu (fokus na Email, Tab na Password, Register in OK, Escape zapre,
  fokus se vrne na glavni meni). Prave prijave in predvajanja nisem mogel
  preizkusiti, ker na Macu ni računa AudioVault.

### M13a: Spotify jedro (12. 10. 2026)

- `apricot-shell/src/spotify.rs` (Windows `spotify_win32.rs`): središče
  (vrstni red Tab Windows: seznam, Open, Back), računi (uporabi, prijava,
  odjava, odstranitev z vprašanjem, Delete), prijava prek brskalnika z
  oknom za prijavo, samodejna povezava aktivnega računa ob zagonu, dogodki
  seje (povezava, prekinitev, prijava, odprta povezava), Spotify povezave v
  neposredni povezavi, Connect obvestila (predvajanje z drugih naprav,
  nadaljevanje na naslednjo skladbo, nedosegljiva skladba), prenos
  glasnosti v obe smeri, Command+PageUp/PageDown, Shift+S (tudi smart
  shuffle) in R prek Connect, nastavitve Spotify (kakovost, normalizacija,
  samodejno predvajanje, vrstni red knjižnice) in uporaba sprememb iz okna
  nastavitev, zaustavitev seje ob izhodu.
- `apricot-ui-macos/src/spotify_dialogs.rs`: okno za prijavo (fokus na
  stanju, Open browser again, Copy login link, Try again samo po napaki,
  Cancel z Escape) in okno nastavitev Spotify.
- Seznami (iskanje, knjižnica, Liked songs, seznami, Home, Daily Mixes,
  nedavno, Top, Browse all) pridejo z M13b, vrsta in naprave z M13c; do
  takrat povedo, da še niso na voljo.
- Preverjeno: samodejni preizkus na Macu (središče z vrstico Log in, Return
  odpre okno za prijavo s fokusom na stanju, Tab čez Open browser, Copy
  link, Cancel, Try again je onemogočen, Escape zapre in pove "Spotify login
  cancelled"). Prave prijave nisem preizkusil, ker potrebuje brskalnik in
  tvoj račun.

### M13b: Spotify seznami (12. 10. 2026)

- `apricot-shell/src/spotify_lists.rs` (Windows `spotify_browse_win32.rs`):
  iskanje, knjižnica in mape, Liked Songs, seznami predvajanja, albumi,
  izvajalci, oddaje, Home in njeni razdelki, Daily Mixes, nedavno
  predvajano, Top, Browse all, radio in profili. Vsak odprt seznam je nova
  raven; Escape in Back se vrneta na isto vrstico prejšnje ravni. Seznam
  se nalaga po straneh, zadnja vrstica naloži naslednjo z "Loading more
  results". Odprt seznam predvajanja se vsakih 15 sekund preveri za
  spremembe od drugod, izbrana pojavitev ostane izbrana in fokus se ne
  premakne. Kontekstni meni z vsemi ukazi Windows (predvajaj, premešaj,
  odpri, dodaj v vrsto, album, izvajalec, kopiraj povezavo, všečkaj ali
  shrani, dodaj v Spotify seznam z New playlist na vrhu, odstrani, premakni
  gor in dol, preimenuj, ustvari, skrij pesem, radio, lastnik, opis,
  vidnost, predogled). Delete odstrani izbrano pojavitev v urejljivem
  seznamu, Command+Shift+Q doda v Spotify vrsto, izbrana vrstica je
  aktivni element za priljubljene in Apricot sezname.
- Okno za iskanje (`spotify_dialogs.rs`): polje, vrsta iskanja, Search,
  Cancel. Prazno iskanje pusti okno odprto s fokusom v polju. Po iskanju se
  fokus ne vrne na prejšnjo kontrolo, ampak ga prevzame seznam rezultatov,
  da bralnik ne prebere vmesne kontrole (točka 4a.8 na Macu).
- Preverjeno: enotni testi (ime seznama in kontrole, vrnitev na isto
  vrstico, meni urejljivega seznama, kontekst in uid predvajanja) in
  samodejni preizkus okna za iskanje (Return na praznem ostane, vnos, Tab
  vrstni red polje, vrsta, Search, Cancel, rezultat). Pravih Spotify
  seznamov nisem preizkusil, ker potrebujejo prijavo v tvoj račun.

### M13c: Spotify vrsta in naprave (12. 10. 2026)

- `apricot-shell/src/spotify_panels.rs` (Windows `show_queue`,
  `show_devices`, `spotify_queue_win32.rs`, `spotify_devices_win32.rs`):
  vrsta s predvajanjem izbrane pojavitve, premikom gor in dol (samo med
  ročno dodanimi), odstranitvijo, brisanjem ročno dodanih in sporočilom
  "The Spotify queue changed meanwhile", kadar urejanje ne uspe. Naprave s
  "Play here", ki ostane odprto in pove zakaj, kadar nič ne igra ali naprava
  že igra. Potrjena vrsta in spremembe naprav posodobijo odprto okno;
  nespremenjen seznam se ne osveži, izbrana ostane ista pojavitev ali
  naprava.
- `apricot-ui-macos/src/spotify_panels.rs`: modalno okno z navodili,
  seznamom (fokus) in gumbi, Back zadnji z Escape. Enter izvede prvi gumb,
  Delete odstrani v vrsti, Shift+F10 odpre meni samo z ukazi, ki jih izbrana
  vrstica podpira. Po zaprtju se fokus vrne na prejšnjo kontrolo.
- Preverjeno: samodejni preizkus vrste z izmišljenimi vrsticami (začetni
  izbor prva naslednja skladba, Tab vrstni red seznam, Play, Move up, Move
  down, Remove, Clear, Back, meni prve ročno dodane skladbe brez Move up,
  Remove brez seje pove, da se je vrsta spremenila, Escape zapre in vrne
  fokus na glavni meni). Prave vrste in naprav nisem preizkusil, ker
  potrebujeta prijavo v tvoj Spotify račun.

### M14a: naročnine in center obvestil (12. 10. 2026)

M14 je razdeljen na dva dela: M14a (naročnine in center obvestil, skupna
logika) in M14b (macOS sistem: element v menijski vrstici, sistemska
obvestila, odpiranje iz Finderja, zagon ob prijavi, privzeti predvajalnik).

- `apricot-shell/src/subscriptions.rs` (Windows `show_subscriptions`,
  `refresh_subscriptions`, preverjanje naročnin, `show_notification_center`):
  zaslon naročnin z vrstnim redom Tab Windows (Back, Check now, Open videos,
  New videos, Remove, Filter category, Set category, seznam), ohranjanje
  izbrane naročnine po osvežitvi, kontekstni meni, Delete, filter in
  kategorija, naročanje in odjava na rezultatih. Preverjanje naročnin na
  lastnem yt-dlp izvajalniku (pet najnovejših videov na kanal), časovnik
  (0,5 do 168 ur), preverjanje ob zagonu in po zaprtju nastavitev, ko je
  čas, "Check subscriptions now" v nastavitvah. Novi videi gredo v center
  obvestil, vsak kanal se oglasi s številom novih, ročno preverjanje pove
  izid.
- Center obvestil: Back, Play, Clear notifications, seznam; Enter predvaja,
  Delete odstrani in ohrani vrstico, kontekstni meni.
- Klici sistemskih obvestil za nove videe, nove epizode in končan prenos v
  ozadju so povezani; prikaz na Macu pride z M14b.
- Preverjeno: enotni testi (vrstni red kontrol, ohranitev izbire, Delete,
  center obvestil) in samodejni preizkus na Macu z resničnim kanalom NPR
  Music prek yt-dlp: samodejno preverjanje ob zagonu je dodalo nove videe v
  center obvestil, "Check now" je povedal "Checking subscriptions" in nato
  "No new subscription videos", po umetno odstranjenem videu pa
  "Subscription check complete" z novim obvestilom.

### M14b: menijska vrstica, obvestila, zagon ob prijavi, privzeti predvajalnik (12. 10. 2026)

- Element v menijski vrstici (`apricot-ui-macos/src/status_item.rs`, Windows
  ikona v pladnju) ostane celo sejo: Show ApricotPlayer, Settings, Check
  subscriptions in Exit ApricotPlayer, besedila v trenutnem jeziku. VoiceOver
  ga doseže z VO+M dvakrat.
- Zapiranje okna (gumb ali novi Command+W v meniju Window) z vklopljeno
  nastavitvijo skrije okno in pove "ApricotPlayer is still running in the
  menu bar."; z nastavitvijo za obvestilo pokaže še sistemsko obvestilo.
  Show vrne okno s fokusom tam, kjer je bil.
- Sistemska obvestila (`notifications.rs`, User Notifications): novi videi,
  nove epizode, končan prenos v ozadju in skrivanje v menijsko vrstico;
  macOS ob prvem obvestilu vpraša za dovoljenje. Delujejo samo iz paketa
  `.app` (M15), razvojni zagon iz terminala jih ne pokaže.
- Zagon ob prijavi: agent v `~/Library/LaunchAgents/com.apricotplayer.*.plist`,
  ki odpre paket `.app`, ob shranjevanju nastavitev kot na Windows.
- Privzeti predvajalnik (`default_player.rs`): Launch Services nastavi
  ApricotPlayer kot pregledovalnik za pogoste zvočne in video vrste; deluje
  samo iz paketa, vrste mora paket razglasiti v `Info.plist` (M15).
- Besedila, ki na Windows omenjajo pladenj, Windows ali Ctrl, imajo na Macu
  svojo različico (`ključ@macos` v `rust_strings.json`, vseh 27 jezikov):
  menijska vrstica, obvestila macOS, zagon ob prijavi, Command namesto Ctrl
  v navodilih vrste prenosov, Keychain v namigu za piškotke. Windows
  besedila ostanejo nespremenjena.
- Preverjeno: enotni testi (besedila po platformah, agent za zagon) in
  samodejni preizkus na Macu (Command+W skrije okno in pove besedilo za
  menijsko vrstico, meni elementa ima pravilne vnose, Show vrne okno s
  fokusom na glavnem meniju). Sistemskih obvestil, zagona ob prijavi in
  privzetega predvajalnika iz paketa nisem mogel preizkusiti, ker paket še
  ne obstaja (M15).

### M15: paket .app in DMG (12. 10. 2026)

- `rust/scripts/build_macos_app.sh` zgradi `ApricotPlayer 2 Beta.app` za
  Apple silicon v `rust/local-dist/macos/<verzija>`: izvršljiva datoteka
  `ApricotPlayer2Beta`, `mpv/` (libmpv, ukaz mpv in vse knjižnice Homebrew,
  prevezane z dylibbundler na `@loader_path`, brez sklicev na Homebrew),
  `ffmpeg/ffmpeg`, `components/yt-dlp` (uradni yt-dlp_macos 2026.08.19,
  preverjen SHA-256), `components/apricot-youtube-helper`, `node/node`
  (Node.js v24.21.0 LTS, preverjen SHA-256), zvok in `build-info.json` v
  `Contents/Resources` (podatki ne smejo biti ob podpisani kodi).
- `rust/packaging/macos/Info.plist`: identiteta
  `com.apricotplayer.ApricotPlayer2Beta`, vrste dokumentov za zvok in video
  (vloga Viewer, Alternate), uvožene vrste za ogg, opus, mkv in webm, opis
  za Apple Events (zapiranje brskalnika pred kopiranjem piškotkov),
  najmanj macOS 12.
- Ad hoc podpis vsake knjižnice in paketa (`codesign --verify --strict`
  gre skozi), preizkus zagona (`--qualification-smoke`,
  `--qualification-ytdlp`, ffmpeg in mpv), nato DMG z bližnjico do
  Applications, zip za posodobitve (M16) in `SHA256SUMS.txt`.
- Ker podpis ni Applov, macOS ob prvem odprtju prenesenega paketa opozori;
  odpre se z desnim klikom in Open ali v System Settings, Privacy &
  Security, Open Anyway. To mora pisati v navodilih za izdajo.
- Ikone še ni (repozitorij nima izvorne slike); paket ima privzeto ikono.
- Preverjeno: gradnja, podpis, DMG se priklopi s paketom in bližnjico,
  zapakiran program s praznim PATH predvaja zvočno datoteko z
  vgrajenim libmpv (lsof: 50 knjižnic iz paketa, 0 iz Homebrew).

### M16: posodobitve na Macu (12. 10. 2026)

- `apricot-shell/src/updates.rs` (Windows `start_ytdlp_update_check`,
  `start_app_update_check`, `offer_app_update`, `begin_app_update_install`,
  `finish_app_update_install`): preverjanje ob zagonu (yt-dlp po 3,5 s,
  program po 5,5 s), časovnik za program (0,5 do 24 ur), ročna preverjanja
  v nastavitvah, vrstica "Update available" v glavnem meniju, obvestilo v
  ozadju, okno "Update available" (besedilo sprememb s fokusom na vrhu,
  Update now, Skip this version z Escape), napredek prenosa, preskočena
  različica in dnevnik `updater.log`.
- Posodabljalnik pozna paket za Mac (`ApricotPlayer2Beta-macos-arm64.zip`
  z enim paketom `.app`, brez namestitvenega programa) in yt-dlp za Mac
  (`yt-dlp_macos` iz uradne izdaje, preverjen SHA-256 in Mach-O glava,
  shranjen kot `components/yt-dlp` z dovoljenjem za zagon).
- Namestitev: skripta `/bin/sh` počaka, da se program zapre, znova preveri
  SHA-256, razširi zip z `ditto`, preveri podpis novega paketa, odmakne
  stari paket, postavi novega in ga zažene z `--updated-relaunch`; ob
  napaki vrne stari paket. Iz razvojne gradnje izven paketa namestitve ne
  zažene.
- Za izdajo beta 6: različica v `Cargo.toml` mora biti 2.0.0-beta.6, izdaja
  na GitHubu mora poleg Windows sredstev imeti
  `ApricotPlayer2Beta-macos-arm64.zip` (posodobitve) in DMG (prva
  namestitev).
- Preverjeno: enotni testi (izbira sredstva in postavitev zipa za Mac,
  posodobitev yt-dlp, skripta zamenja paket in ob spremenjenem paketu
  pusti starega) ter celoten preizkus z zapakiranim programom in lokalnim
  virom izdaj: ob zagonu se je odprlo okno "Update available" s fokusom na
  besedilu sprememb, klik na Update now je prenesel in preveril paket,
  program se je zaprl, skripta je zamenjala paket in ga znova zagnala.
  Ročno preverjanje yt-dlp proti GitHubu pove, da je 2026.08.19 najnovejši.

### M17: uvoz podatkov iz Windows (12. 10. 2026)

- Nastavitve, General, takoj za "Set default player" (samo na Macu):
  "Import data from Windows". Odpre okno za izbiro mape s podatki
  (`%APPDATA%\ApricotPlayer` Python verzije ali `ApricotPlayer2Beta` Rust
  bete, lahko tudi mapa nad njima) ali ZIP z njimi, vpraša za potrditev,
  shrani trenutne podatke Maca v `backup-before-windows-import-<čas>` v
  mapi podatkov, uvozi in program zapre, da ob naslednjem zagonu prebere
  uvožene datoteke.
- Uvoz vzame trajne uporabniške datoteke iz manifesta podatkov
  (nastavitve, priljubljeni, zaznamki, zgodovina, naročnine, podcasti,
  seznami, obvestila, položaji, vrsta, zadnja seja, arhiv prenosov). Iz
  nastavitev odstrani vse, kar velja samo za računalnik z Windows (mapo
  prenosov in predpomnilnika, zvočno napravo, datoteke in profile piškotkov,
  pot do FFmpeg ter vsako vrednost, ki je pot Windows), zato se uporabijo
  privzete vrednosti Maca. Poverilnic, zaščitenih z Windows (Spotify,
  AudioVault), ne uvozi. Če katera datoteka JSON ni veljavna, ne zamenja
  ničesar.
- Besedila v vseh 27 jezikih.
- Preverjeno: enotni testi (odstranitev poti in naprave iz nastavitev,
  uvoz iz mape z varnostno kopijo in brez poverilnic, poškodovana datoteka
  ne zamenja ničesar, ZIP najde mapo s podatki) in število kontrol v
  nastavitvah na obeh platformah. Okna za izbiro na Macu nisem preizkusil s
  pravimi podatki iz Windows, ker jih tu ni.

### M18 in S1 (12. 10. 2026)

- M18: delovni tok `Rust macOS package` zgradi paket, DMG in zip z
  `build_macos_app.sh` na GitHubovem Macu in ju shrani kot artefakt; teče
  ob spremembi pakiranja ali ročno, ničesar ne objavi. Prevzem z
  VoiceOverjem na Urhovem MacBooku ostaja zanj.
- S1: vrstica "Followed artists" v Spotify meniju takoj pod Playlists, na
  Windows in Macu. Odpre izvajalce iz knjižnice (to so izvajalci, ki jim
  račun sledi) kot vsak drug Spotify seznam; Enter odpre izvajalca.
  Besedilo v 27 jezikih. Preverjeno z enotnim testom vrstnega reda menija;
  s pravim računom ni preizkušeno.
