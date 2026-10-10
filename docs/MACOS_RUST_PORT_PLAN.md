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
- **M2. Izločitev aplikacijske logike iz `win32.rs`.** Po korakih, vsak korak
  posebej preverjen:
  M2a predvajanje (razreševanje, stream cache, prefetch, začetek iz vrste,
  ozadje), M2b prenosi, pretvorbe, posodobitve in diagnostika, M2c YouTube
  seznami, metapodatki, naročnine, podcasti, M2d pogledi, navigacija in fokus
  (model `MainView`, povratne poti, Tab vrstni red kot podatek), M2e AudioVault in
  Spotify stanje. Win32 ostane enak po vedenju; testi se premaknejo v skupni
  crate, da jih poganja tudi macOS. Pri tej enoti potrebujem Urhov kratek NVDA
  preizkus na Windows po vsakem koraku.

### Del B: dokaz najtežjih macOS poti

- **M3. AppKit in VoiceOver spike.** Majhno okno: seznam z začetnicami, gumbi,
  polje za iskanje, potrditveno polje, drsnik, meni, kontekstni meni na Shift+F10
  in VO+Shift+M, oglas, obnova fokusa po dialogu, ter libmpv zvok in video v
  `NSView`. Vrata: Urh potrdi z VoiceOverjem, da so imena, vloge in oglasi
  pravilni, in da video deluje. Če AppKit prek `objc2` ne izpolni pogodbe, se
  ustavim in predlagam alternativo.

### Del C: zasloni in funkcije na macOS

- **M4. Ogrodje `apricot-ui-macos`.** Življenjski cikel aplikacije, glavno okno,
  glavni meni, menijska vrstica macOS, izbira jezika ob prvem zagonu, preslikava
  bližnjic, zadrževane tipke, Action Finder.
- **M5. Predvajalnik.** Zvok in vgrajen video, celozaslonski način, izhodne
  naprave, glasnost, hitrost, višina tona, EQ, ReplayGain, BPM, iskanje po času,
  ozadno predvajanje, oglasi predvajalnika.
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

- **M15. Paket.** `.app` s pribitimi dylibi (libmpv, FFmpeg in odvisnosti),
  yt-dlp, Node, yt-dlp-ejs, ikona, ad-hoc podpis, DMG, skripta za lokalno gradnjo.
- **M16. Posodobitve.** Izbira DMG sredstva za arm64 in kanal, preverjanje
  zgoščene vrednosti, zamenjava `.app`, ponovni zagon, povrnitev; posodabljanje
  yt-dlp in pomočnika.
- **M17. Uvoz podatkov z Windows.** Uvoz mape ali ZIP-a Python ali Rust Windows
  podatkov, brez strojno vezanih poti, naprav in DPAPI poverilnic.
- **M18. CI in prevzem.** GitHub Actions za Windows in macOS (test, clippy),
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
