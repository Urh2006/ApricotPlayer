# Stanje Rust porta ApricotPlayerja

Datum revizije: 27. september 2026.
Vir resnice: Python `main` fe62a21 (1.0.21).
Revidirano stanje Rust: lokalna veja `rust-2.0` na f904192, skupaj z necommitanim
delom za lyrics in transcript.

Ta dokument nadomešča kljukice v `docs/RUST_PARITY_MANIFEST.md` kot oceno
dejanskega stanja. Postavka v manifestu, označena z `[x]`, ni nujno enaka Python
verziji. Podrobni revizijski zapisi s sklici na vrstice v obeh verzijah so v
`docs/rust-audit/`. Oznake v oklepajih, na primer (PLAYER2-02), kažejo na
ustrezno ugotovitev v teh zapisih.

## 0. Povzetek

- Rust projekt se zgradi, 399 testov gre skozi (6 jih je izključenih), `cargo clippy` je čist.
- Arhitektura: lasten Win32 UI z uradnim `windows` crateom, predvajanje z libmpv v
  procesu, yt-dlp kot zunanji proces, besedila so ob gradnji vgrajena iz Pythonovih
  `apricot/locales/*.json` (vseh 27 jezikov).
- Dobro portano: tabela 91 bližnjic, vrstni red in prilagajanje glavnega menija,
  shema nastavitev z ohranjanjem neznanih ključev, EQ prednastavitve in filtrski niz,
  dinamično nalaganje rezultatov, Trending, podcasti in RSS, jedro prenosov,
  bookmarks, resume pozicije, vrstni red Tab v predvajalniku, uvoz Python podatkov.
- Največja težava: 16 od 91 registriranih dejanj nima izvedbe. Bližnjica ali gumb
  odpre modalno angleško sporočilo "is registered, but its Rust route is not
  implemented". To velja tudi za gumbe v predvajalniku (izenačevalnik, izhodne
  naprave, komentarji, način urejanja) in za skoraj vse gumbe v nastavitvah.
- Manjkajoči večji sklopi: AudioVault, updater, SoundCloud, piškotki, pretvornika,
  diagnostično poročilo, BPM, komentarji, izenačevalnik iz predvajalnika, izbira
  izhodne naprave, način urejanja.

## 1. Odstopanja od cilja 1:1 v GPT-jevem planu

1. **Dodatni Rust YouTube backend.** GPT je dodal "Rusty YTDL" z novo nastavitvijo
   `youtube_backend`, lastnim pomožnim programom `apricot-youtube-helper` in
   dodatnim poljem v razdelku General (SEARCH-09, SETTINGS-04). Python tega nima.
   Polje premakne vrstni red Tab v razdelku General.
2. **Sprememba Python datoteke.** Veja `rust-2.0` je v `apricot/locales/en.json`
   dodala ključe `ok`, `cancel`, `youtube_backend*`, `direct_link_invalid`,
   `direct_link_fallback` in `check_youtube_component_updates_now`. Drugi jeziki jih nimajo.
3. **Namerne spremembe vmesnika.** V planu so sprejete kot izboljšave, v kodi pa se
   kažejo kot odstopanja. Primeri: Save zapre nastavitve, Reset all ima dodatno
   potrditveno okno, kontekstni meniji imajo dodatne ali drugače razvrščene postavke,
   mapa se nalaga po 20 datotek.
4. **Vrstni red faz.** Plan je predvideval, da se predvajalnik (faza 3) konča pred
   YouTube, podcasti in prenosi. GPT je nadaljeval s fazami 5, 7 in 8, preden je bil
   predvajalnik zaključen. Zato velik del predvajalnika še manjka.
5. **Označevanje zaključenosti.** Manifest ima nekaj postavk označenih kot
   zaključene brez NVDA preizkusa. Revizija je pri teh našla odstopanja.
6. **Ločen podatkovni imenik.** Rust beta uporablja `%APPDATA%\ApricotPlayer2Beta`
   (odločitev D-011). Ob prvem zagonu enkrat prekopira vsako Python datoteko, ki je
   v beta imeniku še ni. Python podatkov nikoli ne spreminja in poznejših sprememb
   v Pythonu ne prevzame. To je skladno z zahtevo, da Rust prebere Python podatke.

## 2. Odstopanja v že portanih delih

Prioriteta: P1 pomeni, da je osnovna uporaba s tipkovnico ali NVDA zlomljena ali zmedena. P2 je opazna razlika, P3 kozmetična.

### Globalno

| ID | P | Odstopanje |
|---|---|---|
| SHELL-01 | P1 | 16 dejanj pade na angleško modalno sporočilo namesto izvedbe: `open_audiovault`, `open_channel`, `copy_diagnostic_report`, `player_bpm`, `player_comments`, `player_edit_mode`, `player_equalizer`, `player_fullscreen`, `player_next_related`, `player_output_devices`, `player_replace_edit_original`, `player_replaygain`, `player_save_edit_copy`, `player_shuffle`, `result_column_previous`, `result_column_next`. Enako se zgodi pri pretvornikih iz glavnega menija. |
| SHELL-02 | P2 | Ob prvem zagonu, skritem v pladnju, se vseeno odpre okno za izbiro jezika. |
| SHELL-03 | P3 | Prvi zagon ne preverja, ali datoteka z nastavitvami sploh obstaja (Python `first_run_without_settings`). |
| SHELL-M-04 | P2 | Oglaševanje nima poti za JAWS, dogodka `EVENT_SYSTEM_ALERT` in `VALUECHANGE` na statusni kontroli; uporablja le NVDA in `NAMECHANGE`. |

### Predvajalnik

| ID | P | Odstopanje |
|---|---|---|
| PLAYER2-01 | P1 | Next, Previous in Related v Rustu ohranijo hitrost, višino tona, izhodno napravo in EQ prejšnjega posnetka. Python vsak posnetek zažene z nastavljeno privzeto hitrostjo in višino 1.0 in ohrani samo glasnost. |
| PLAYER2-02 | P1 | Oglasi za T, V, S/D, Ctrl+gor/dol, Ctrl+0, R, volume boost in bass boost so trdo kodirani v angleščini in drugače oblikovani (npr. "Speed 1.25" namesto "Playback speed 1.3x.", "Speed and pitch reset" brez vrednosti, "Elapsed x" namesto "Timing is not available yet."). |
| PLAYER-03 | P1 | Kontekstni meni predvajalnika nima postavk za izhodne naprave, celozaslonski način, izenačevalnik, ReplayGain, shranjevanje hitrosti podcasta, sorodni video, dodajanje zaznamka, seznam zaznamkov, poglavja, prepis, besedilo pesmi, komentarje in odpiranje v brskalniku. Namesto tega ima dodatni postavki za podrobnosti in vrsto, dodajanje na playlist pa ni podmeni obstoječih playlistov. |
| PLAYER2-08 | P2 | Nastavitev "Audio quality when changing speed" (rubberband, scaletempo2, scaletempo, mpv) se ne uporabi; mpv vedno uporabi privzeti algoritem. Treba je preveriti tudi `pitch_mode` pri spremembi višine tona (PLAYER2-M-09). |
| PLAYER2-06 | P2 | Odpravljeno v E7: podrobnosti so vgrajene v predvajalnik in se ob nastavitvi odprejo same. |
| PLAYER2-05 | P2 | Pri background playback Python vključi seznam rezultatov v Tab vrstni red predvajalnika, Rust ne. |
| PLAYER-02 | P3 | Pot pri neuspelem nalaganju je treba preveriti med izvajanjem (oglas `player_failed`). |

### Iskanje in rezultati

| ID | P | Odstopanje |
|---|---|---|
| SEARCH-01 | P1 | Iskalni zaslon nima izbire ponudnika SoundCloud in tipov Track/Playlist/User. |
| SEARCH-08 | P2 | Iskalni zaslon nima gumbov Play, Download audio, Download video in Add favorite; to spremeni Tab vrstni red. |
| SEARCH-02 | P2 | Kontekstni meni rezultatov nima "Open in browser". |
| SEARCH-05 | P2 | Meni kanala ima dodaten podmeni za prenos in drugačen vrstni red. |
| SEARCH-06 | P2 | Meni videa nima `remove_from_playlist` in `open_channel`, vrstni red je drugačen, dodana je postavka za vrsto. |
| SEARCH-03 | P3 | "Copy link" namesto "Copy URL". |
| SEARCH-04 | P3 | Vrstica playlista nima števila videov. |

### Knjižnica

| ID | P | Odstopanje |
|---|---|---|
| LIBRARY-M-01 | P1 | Enter na kanalu ali playlistu v priljubljenih ali zgodovini pokaže angleško napako namesto odprtja. |
| LIBRARY-01 | P2 | Mapa se naloži po 20 datotek; Python naloži celo mapo naenkrat (preverjeno v `apricot/ui/misc.py:1304-1310`). |
| LIBRARY-02 | P2 | Meniji priljubljenih in zgodovine ne ločijo lokalnih in spletnih elementov, manjkajo `remove_from_playback_queue`, `copy_stream_url`, `open_channel`, napačen ključ `copy_link` namesto `copy_path` ali `copy_url`. |
| LIBRARY-03 | P3 | Meni seznama playlistov ima tri dodatne postavke in drugačen vrstni red. |

### Nastavitve

| ID | P | Odstopanje |
|---|---|---|
| SETTINGS-01 | P1 | Deluje samo gumb za ponastavitev razdelka. Browse, Set default player, preverjanje posodobitev, piškotki, API ključ, AudioVault, EQ profili in "Check subscriptions now" pokažejo angleško sporočilo "not implemented". |
| SETTINGS-02 | P2 | Save zapre okno in ne oglasi "settings saved"; Python ostane na zaslonu. |
| SETTINGS-03 | P2 | Gumb za vse nastavitve se imenuje "Reset all settings" namesto "Restore to defaults" in ima dodatno angleško potrditveno okno. |
| SETTINGS-04 | P2 | Dodatno polje YouTube backend v razdelku General. |
| SETTINGS-05 | P2 | Sporočilo o konfliktu bližnjice je v angleščini in izpiše notranji ID dejanja. |
| SETTINGS-06 | P3 | Preklop razdelkov nima zamika 140 ms, zato se kontrole gradijo ob vsaki puščici. |

### Podcasti in prenosi

| ID | P | Odstopanje |
|---|---|---|
| PODDL-01 | P2 | Bližnjica za prenos nima zaščite 0,35 s pred dvojnim pritiskom. |
| PODDL-03 | P2 | Vrstica epizode nima oznake, da je v vrsti za prenos. |
| PODDL-02 | P3 | Napredek prenosa zvoka ni omejen po pogostosti. |

## 3. Manjkajoče funkcije Python verzije

| ID | P | Funkcija | Python |
|---|---|---|---|
| PLAYER2-M-02, SETTINGS-M-02 | P1 | Izenačevalnik iz predvajalnika (F4) ter ustvarjanje, uvoz, izvoz, brisanje in ponastavitev EQ profilov | `apricot/ui/equalizer.py`, `apricot/ui/settings.py:762-846` |
| PLAYER2-M-03 | P1 | Odpravljeno v E9: izbira izhodne naprave (O) | `apricot/player/volume.py`, `apricot/ui/player.py` |
| PLAYER2-M-05 | P1 | Odpravljeno v E8: preklop shuffle (Shift+S), sorodni video (Ctrl+Shift+PageDown), ReplayGain (Ctrl+Shift+G), celozaslonski način (F11) | `apricot/ui/misc.py:1358`, `apricot/ui/player.py` |
| PLAYER2-M-10 | P2 | Predvajanje v ozadju (`enable_background_playback`, privzeto izklopljeno): Back pusti predvajanje teči in odpre glavni meni, predvajalnik ima vgrajen seznam rezultatov (PLAYER2-05), v celozaslonskem načinu pa gumb "Back to results". V Rustu vsi gumbi Back in Close player predvajanje ustavijo. Ugotovljeno iz kode med E8. Ni del E9, dodano kot enota E13a. | `apricot/ui/player.py:648-812`, `apricot/ui/events.py:605-620` |
| PLAYER2-M-01 | P1 | Odpravljeno v E11: BPM analiza (B) | `apricot/ui/misc.py:2710-2803` |
| PLAYER2-M-04 | P1 | Komentarji (Ctrl+Shift+M) | `apricot/ui/misc.py:2095+` |
| PLAYER2-M-06 | P1 | Način urejanja (E, Ctrl+S, Ctrl+R) | `apricot/ui/misc.py:2389-2433` |
| SEARCH-M-01 | P1 | SoundCloud iskanje, izvajalci in seti | `apricot/search/search.py:451-474, 703-729` |
| SEARCH-M-02 | P1 | Odpri kanal (Ctrl+Shift+O) in stolpci rezultatov (Ctrl+Alt+levo/desno) | `apricot/search/search.py:233` |
| SEARCH-M-04 | P2 | Vmešavanje Shorts v iskanje in v objave kanala | `apricot/search/search.py:374-495` |
| SHELL-M-03 | P1 | Kopiranje diagnostičnega poročila | `apricot/system/diagnostics.py` |
| SETTINGS-M-01 | P1 | Piškotki: datoteka, uvoz iz brskalnika, DevTools izvoz, prijavni profil | `apricot/ui/cookies.py` |
| SETTINGS-M-03 | P2 | Set default player in pomoč pri povezavah datotek | `apricot/ui/settings.py:462` |
| PODDL-M-01 | P1 | Pretvornik datotek in pretvornik map | `apricot/media/media.py:40-436` |
| PODDL-M-03, SHELL-M-02 | P1 | yt-dlp posodobitve ter posodobitve aplikacije s kanali, preskokom verzije, preverjanjem in rollbackom | `apricot/updater/updater.py` |
| PODDL-M-02, SHELL-M-01 | P1 | AudioVault v celoti | `apricot/network/audiovault.py` |

Action Finder, pladenj, center obvestil in obnovitev fokusa v glavnem meniju je pregledala
E6, naročnine, vrsto predvajanja, neposredno povezavo, podrobnosti (F7) in poglavja pa E7.
Nepregledana ostajajo natančna besedila napak pri iskanju.

## 4. Delovne enote

Vsaka enota se začne s ciljano primerjavo Python kode za svoje področje. Konča se s
`cargo build`, `cargo test`, `cargo clippy`, avtomatiziranim testom, kjer je izvedljiv,
in kratkim ročnim NVDA preizkusom. Pri predvajalniku enota preveri tudi dejansko mpv pot.
Enote, ki popravljajo odstopanja, so na vrsti prve.

### A. Popravki obstoječih odstopanj

- **E1. Oglasi in seja predvajalnika. Zaključeno 27. 9. 2026, glej razdelek 6.**
- **E2. Enotna pot za dejanja, ki še niso narejena. Zaključeno 27. 9. 2026, glej razdelek 6.**
- **E3. Kontekstni meniji. Zaključeno 27. 9. 2026, glej razdelek 6.**
- **E4. Nastavitve, prvi del. Zaključeno 27. 9. 2026, glej razdelek 6.**
- **E5. Seznami in knjižnica. Zaključeno 27. 9. 2026, glej razdelek 6.**
- **E6. Lupina in oglaševanje. Zaključeno 28. 9. 2026, glej razdelek 6.**
- **E7. Revizija in popravki preostalih zaslonov. Zaključeno 28. 9. 2026, glej razdelek 6.** Naročnine, vrsta predvajanja,
  neposredna povezava, podrobnosti in poglavja ter trenutno necommitano delo za
  lyrics in transcript. Python podrobnosti (F7) niso dialog, ampak vgrajeno polje
  za branje z gumboma Copy details in Back na zaslonu predvajalnika. Rust jih ima
  kot dialog, zato sem vanjo prestavil tudi PLAYER2-06 (samodejno odpiranje
  podrobnosti) in sprotno posodabljanje hitrosti in višine tona v podrobnostih.

### B. Majhne manjkajoče funkcije predvajalnika

- **E8. Shuffle, sorodni video, ReplayGain cikel in celozaslonski način. Zaključeno 28. 9. 2026, glej razdelek 6.**
- **E9. Izbira izhodne naprave (O) z osvežitvijo seznama in varnim nadomestkom. Zaključeno 28. 9. 2026, glej razdelek 6.**
- **E10. Izenačevalnik iz predvajalnika (F4) ter EQ profili v nastavitvah. Zaključeno 28. 9. 2026, glej razdelek 6.**
- **E11. BPM analiza. Zaključeno 28. 9. 2026, glej razdelek 6.**
- **E12.** Način urejanja z varnim shranjevanjem kopije in zamenjavo izvirnika.
- **E13.** Komentarji.
- **E13a.** Predvajanje v ozadju (PLAYER2-M-10): nastavitev `enable_background_playback`,
  Back brez ustavitve predvajanja, vgrajen seznam rezultatov v predvajalniku in gumb
  "Back to results" v celozaslonskem načinu. Dodano po E8, vrstni red lahko Urh spremeni.

### C. Večji manjkajoči sklopi

- **E14.** SoundCloud, Shorts vmešavanje in gumbi na iskalnem zaslonu.
- **E15.** Diagnostično poročilo z zakrivanjem zasebnih podatkov.
- **E16.** Piškotki.
- **E17.** Pretvornik datotek in pretvornik map.
- **E18.** yt-dlp posodobitve in posodobitve aplikacije. Pred objavo 2.0 ostanejo
  po D-011 lokalno onemogočene.
- **E19.** AudioVault.
- **E20.** Zaključna parity vrata: ponovna primerjava manifesta, preverjanje števil
  v registrih, NVDA preizkus celote, preverjanje uvoza podatkov in zmogljivosti.

## 5. Odobrene odločitve (27. 9. 2026)

1. Polje YouTube backend se odstrani iz nastavitev, vedno se uporablja yt-dlp.
   Koda pomožnega programa ostane v repozitoriju neuporabljena.
2. Ključi, ki jih je GPT dodal v Pythonov `en.json`, se premaknejo na Rust stran,
   `en.json` pa se vrne v stanje iz `main`.
3. Rust beta ostane v ločenem imeniku z enkratnim uvozom Python podatkov.

### Odobrena odstopanja od Pythona

Kadar je Pythonovo vedenje očitna majhna napaka ali pozaba, sme biti Rust boljši. Tako
odstopanje se najprej predlaga Urhu in se po odobritvi zapiše sem.

- **O-1.** Delujoč neposreden klic JAWS (`SayString`), ki ga Python poskuša, a mu ne
  uspe (dopolnitev E6).
- **O-2.** Po vrnitvi iz predvajalnika v center obvestil ali zgodovino ostane izbrana
  predvajana vrstica namesto prve vrstice (dopolnitev E6).
- **O-3.** Iskanje v prepisu med nalaganjem: Python seznam zamenja z "No transcript or
  captions available.", čeprav se prepis še nalaga. Rust ohrani "Loading transcript"
  (predlog P-1 iz E7, odobren 28. 9. 2026).
- **O-4.** Neposredna povezava, ki tudi z dodanim `https://` ni veljaven naslov (na primer
  presledki v imenu strežnika): Python jo poskusi predvajati in javi napako yt-dlp, Rust
  takoj oglasi `direct_link_invalid` (predlog P-2 iz E7, odobren 28. 9. 2026).
- **O-5.** Ko ročni sorodni video (Ctrl+Shift+PageDown) ne najde ničesar, Python poleg
  oglasa "No related video available." označi predvajanje kot končano in gumb Pause
  preimenuje v Play, čeprav posnetek teče naprej. Rust samo oglasi sporočilo (predlog iz
  E8, odobren 28. 9. 2026).
- **O-6.** Konec posnetka: Python oglasi "Playback finished." samo, ko samodejni sorodni
  video ne najde ničesar. Ob navadnem koncu (z ali brez "autoplay next") je tiho, zato
  nastavitev "Announce when playback finishes" skoraj nima učinka. Rust (že pred E8) ob
  vsakem koncu brez naslednjega elementa oglasi "Playback finished.", če je nastavitev
  vklopljena (predlog iz E8, odobren 28. 9. 2026).

## 6. Dnevnik enot

### E1: oglasi in seja predvajalnika (27. 9. 2026)

Spremembe:

- Next, Previous, Related in drugi novi posnetki v odprti seji dobijo nastavljeno
  začetno hitrost (ali hitrost podcasta) in višino tona 1.0. Glasnost, izhodna
  naprava, EQ in preklopi ostanejo za sejo, tako kot v Pythonu (PLAYER2-01).
- Ker Rust ohrani eno libmpv instanco, runtime pred zamenjavo posnetka pošlje
  stanje, ki ga Python dobi z novim mpv procesom: pavza glede na
  `player_start_paused`, meja in vrednost glasnosti, `audio-pitch-correction`,
  hitrost, višina tona, ponavljanje in celoten filtrski niz. Prej je nov posnetek
  podedoval tudi pavzo prejšnjega.
- `speed_audio_mode` in `pitch_mode` se uporabita kot v Pythonu: filter
  `@apricot_speed`, `audio-pitch-correction`, Rubberband filter `@apricot_pitch`
  za načina Rubberband in povezano hitrost, pri povezanem načinu pa tipke za višino
  tona spremenijo tudi hitrost (PLAYER2-08).
- Oglasi T, V, S in D, Ctrl+gor/dol, Ctrl+0, Ctrl+Home, Ctrl+End, R, volume boost,
  bass boost in samodejni naslednji uporabljajo Pythonove ključe in oblike
  (PLAYER2-02). Dodana sta oglasa "Jumped to start." in "Jumped to end.", skok na
  začetek in konec je natančen, konec pa je 0,5 s pred koncem.
- Tipki gor in dol spremenita glasnost brez oglasa, kot v Pythonu. Rust je prej
  vsakič prebral glasnost.
- Ob doseženi hitrosti ali višini tona 1.0 in ob Ctrl+0 se predvaja
  `assets/default_reached.wav`. Skript za lokalni beta paket zdaj kopira to datoteko.
- Napake ob zagonu predvajalnika uporabljajo lokaliziran `player_failed` (PLAYER-02).

Preverjanje: `cargo build`, `cargo test` (406 uspešnih, 8 izključenih), `cargo clippy
-D warnings` in `cargo fmt --check` gredo skozi. Nov test z dejanskim libmpv
(`real_libmpv_accepts_python_speed_pitch_and_equalizer_chains`) predvaja posnetek z
vsemi 12 kombinacijami načinov hitrosti in višine tona skupaj z EQ filtrom in
potrdi, da libmpv neveljaven filter zavrne. Zaženeš ga z nastavljenima
`APRICOT_TEST_MPV` in `APRICOT_TEST_FFMPEG` ter zastavico `--ignored`.

Odprto za poznejše enote: EQ spremembe zdaj ponastavijo celoten filtrski niz z
ukazom `set af`. Python dodaja in odstranjuje samo EQ filter z oznako. To bo
treba poenotiti v E10 skupaj z izenačevalnikom. Napaka posameznega ukaza mpv se v
Rustu še vedno pokaže kot okno "Player did not start" namesto oglasa
"Timing is not available yet.", kar ostaja za E2.

### E2: enotna pot za dejanja, ki še niso narejena (27. 9. 2026)

Spremembe:

- Nobeno dejanje brez Rust izvedbe ne odpre več angleškega modalnega okna. Namesto
  tega se v vrstici stanja in govoru oglasi lokaliziran stavek, na primer
  "Equalizer is not available in this beta yet." Fokus ostane, kjer je bil.
  To velja za 16 bližnjic in gumbov predvajalnika iz SHELL-01 in PLAYER2-04, za
  postavke glavnega menija brez Rust zaslona, za odpiranje kanala ali playlista
  iz priljubljenih in zgodovine ter za ukazne gumbe v nastavitvah (SETTINGS-01).
- Kjer ima Python za isto situacijo svoje sporočilo, se uporabi to: komentarji
  pri posnetku brez YouTube ID ("comments_disabled"), naslednji sorodni posnetek
  pri posnetku, ki ni z YouTuba ("no_related_video"), način urejanja pri spletnem
  posnetku ("edit_mode_local_only") in BPM brez predvajalnika ("bpm_not_available").
  Ctrl+S in Ctrl+R v predvajalniku ostaneta tiha, ker Python brez vklopljenega
  načina urejanja ne naredi ničesar.
- Potrditveno polje Full screen se po neuspelem dejanju vrne v stanje seje.
- Napaka posameznega ukaza mpv med predvajanjem ne ustavi več seje in ne odpre
  okna "Player did not start". Oglasi se "Timing is not available yet.", kot v
  Pythonu. Nov dogodek `PlaybackEvent::CommandFailed` loči to od napake ob zagonu
  ali med predvajanjem, ki še vedno pokaže `player_failed`.
- Besedila, ki jih ima samo Rust, so zdaj v `rust/crates/apricot-app/locales/rust_strings.json`
  za vseh 27 jezikov. Pythonovih besedil nikoli ne nadomestijo. E4 bo sem premaknil
  ključe, ki jih je GPT dodal v Pythonov `en.json`.

Preverjanje: `cargo build`, `cargo test` (410 uspešnih, 8 izključenih), `cargo clippy
-D warnings` in `cargo fmt --check` gredo skozi. Novi testi preverijo besedila in
Pythonova sporočila za posamezna dejanja, tihe primere, pokritost vseh jezikov in
to, da zavrnjen ukaz mpv sporoči `CommandFailed` in ne konča posnetka.

### E3: kontekstni meniji (27. 9. 2026)

Spremembe:

- Kontekstni meniji rezultatov iskanja, Trending, vsebine kanala ali playlista,
  mape, priljubljenih, zgodovine, seznama playlistov, vsebine playlista in
  predvajalnika se zdaj gradijo iz enega modela v `apricot-app/src/context_menu.rs`,
  ki sledi `open_context_menu`, `open_player_context_menu`,
  `open_user_playlists_context_menu`, `open_favorites_context_menu`,
  `open_history_context_menu` in `open_user_playlist_items_context_menu` postavko
  za postavko. Model ni vezan na Win32, zato ga lahko uporabi tudi macOS.
- Ločevanje lokalnih in spletnih elementov kot v Pythonu, "Copy URL" za spletne in
  "Copy path" za lokalne elemente, v predvajalniku "Copy link" (SEARCH-03, LIBRARY-02).
- Dodane manjkajoče postavke: "Open in browser" (SEARCH-02), `remove_from_playlist`
  in `open_channel` pri videih (SEARCH-06), `remove_from_playback_queue` in
  `copy_stream_url` v priljubljenih in zgodovini, obe postavki "Add to favorites"
  in "Remove from favorites" hkrati, kot v Pythonu, ter "Download all as audio" in
  "Download all as video" za Play, kadar sta v vrsti za prenos vsaj dva elementa.
- Meni kanala ima Pythonov vrstni red. Revizija se je pri SEARCH-05 zmotila: Python
  podmeni "Download channel" ima (oznaka `(None, None)`), zato ostane, le na pravem
  mestu za naročnino.
- Meni predvajalnika ima vse Pythonove postavke v istem vrstnem redu (PLAYER-03).
  Postavke za izhodne naprave, celozaslonski način, izenačevalnik, ReplayGain,
  sorodni video in komentarje gredo skozi isto pot kot bližnjice, zato do enot
  E8 do E13 oglasijo "ni na voljo v tej beti" iz E2. Odstranjeni sta Rust postavki
  za podrobnosti in vrsto predvajanja, ki ju Python v tem meniju nima.
- "Add to playlist" je podmeni z imeni playlistov in postavko "Create playlist", kadar
  playlisti obstajajo (PLAYER-M-02). Kot v Pythonu "Create playlist" v podmeniju
  pokliče `add_active_to_playlist`, ki nov playlist ustvari samo, če ga še ni.
- Meni seznama playlistov ima samo štiri Pythonove postavke, brez playlistov pa
  samo "Create playlist" (LIBRARY-03). Meni mape uporablja Pythonov lokalni del
  menija rezultatov; Play folder, Shuffle folder in Add folder to queue ostanejo
  gumbi na zaslonu, kot v Pythonu.
- Oznake imajo bližnjico za tabulatorjem, kot `menu_label_with_shortcut`, in
  upoštevajo nastavitev "show shortcuts in labels". NVDA jo prebere kot bližnjico
  postavke.

Ostaja za poznejše enote: meni SoundCloud kanala (E14), dejansko odpiranje kanala
(E5), meniji naročnin, RSS, podcastov, obvestil in vrste prenosov niso bili del E3
in ostajajo na starem seznamu, dokler jih ne pregleda E6 ali E7.

### E4: nastavitve, prvi del (27. 9. 2026)

Spremembe:

- Save shrani, oglasi "Settings saved." in pusti okno odprto, fokus ostane na gumbu
  (SETTINGS-02). Kot v Pythonu se zaslon ob spremembi jezika zgradi znova in fokus gre
  na seznam razdelkov.
- Gumb se imenuje "Restore to defaults" in nima potrditvenega okna. Nastavitve takoj
  shrani, oglasi "Default settings restored." in postavi fokus na seznam razdelkov
  (SETTINGS-03). Tudi ponastavitev razdelka zdaj takoj shrani in oglasi
  "{razdelek} settings reset.", kot `reset_settings_section`.
- Gumbi Back, Save in Restore to defaults so v vrstnem redu Tab pred seznamom
  razdelkov, kot v Pythonu, kjer je vrstica gumbov dodana prva.
- Konflikt bližnjice pokaže Pythonovo opozorilo `shortcut_in_use` s prevedenim imenom
  drugega dejanja in naslovom `shortcut_in_use_title`, nato isto besedilo izgovori.
  Uspešno zajeta bližnjica oglasi `shortcut_captured`, kar prej ni (SETTINGS-05).
- Preklop razdelkov počaka 140 ms po zadnji puščici, vidne kontrole se uveljavijo
  enkrat. Tab in Enter na seznamu razdelek takoj prikažeta in premakneta fokus na
  prvo kontrolo (SETTINGS-06). Tab in Enter sta prej prestregla `IsDialogMessageW`,
  zato je Enter na seznamu sprožil Save; zdaj ju seznam prejme sam.
- Browse izbere mapo za prenose, jo takoj shrani in vpiše v polje, fokus se vrne na
  Browse, kot `choose_download_folder`.
- Set default player registrira beto kot predvajalnik medijev za trenutnega
  uporabnika, če registracija še ni popolna, in odpre Windows Default apps. Ob napaki
  poskusi nadzorno ploščo Default Programs, sicer pokaže `default_player_settings_failed`
  (SETTINGS-M-03). Beta uporablja svoje ključe `ApricotPlayer2Beta` in ne prepiše
  registracije Python verzije.
- Odstranjeni so polje YouTube backend, nastavitev `youtube_backend` in Rust ključi za
  besedila "YouTube component" (odločitev 1, SETTINGS-04). Oznake so zdaj Pythonove
  `auto_update` in `check_ytdlp_updates_now`. Odstranjen je tudi dodatni gumb Browse
  za mapo predpomnilnika, ki ga Python nima.
- Ključi `ok`, `cancel`, `direct_link_invalid` in `direct_link_fallback` so v
  `rust_strings.json` za vseh 27 jezikov, `apricot/locales/en.json` je spet enak `main`
  (odločitev 2).

Preverjanje: `cargo build`, `cargo test` (425 uspešnih, 8 izključenih), `cargo clippy
-D warnings` in `cargo fmt --check`. Novi testi preverijo postavitev registracije
predvajalnika in ločitev bete od stabilne verzije, števila kontrol v razdelkih General
in Playback ter oznako za yt-dlp. Samodejni UI Automation preizkus na ločeni kopiji
podatkov je potrdil vrstni red Tab, Enter na seznamu, zamik, Save brez zapiranja in
Restore to defaults brez okna.

Ostaja: preostali ukazni gumbi nastavitev (posodobitve, piškotki, AudioVault, EQ
profili, naročnine) oglasijo "ni na voljo v tej beti" do enot E10, E16, E18 in E19.
Dopolnitev po Urhovi odločitvi: Back, zapiranje okna in dejanje iz Action Finderja
nastavitev ne shranijo in ne zavržejo več. Kot `back_from_settings` ostanejo spremembe
razdelkov, ki jih je uporabnik zapustil, uveljavljene v pomnilniku do naslednjega
shranjevanja ali ponovnega zagona, spremembe v trenutno vidnem razdelku pa se ne
uveljavijo.

### E5: seznami in knjižnica (27. 9. 2026)

Spremembe:

- Predvajanje iz mape pokaže vse datoteke naenkrat, kot `show_local_media_folder`
  (LIBRARY-01). Paketi po 20 in angleško besedilo "files loaded" so odstranjeni.
- Enter na kanalu ali playlistu v priljubljenih ali zgodovini odpre njegove videe, kot
  `open_library_item` (LIBRARY-M-01). Kanal odpre zavihek Videos brez okna z možnostmi,
  Back vrne na priljubljene ali zgodovino. Tak element ne postane zaporedje predvajanja.
  SoundCloud kanal še vedno oglasi "ni na voljo" do E14.
- Odpri kanal (Ctrl+Shift+O in postavka v kontekstnem meniju) odpre videe kanala, ki je
  naložil izbrani video, sicer videe kanala trenutno predvajanega elementa. Brez kanala
  oglasi `no_channel`, kot `open_item_channel` (SEARCH-M-02).
- Ctrl+Alt+desno in Ctrl+Alt+levo na seznamu prebereta naslednje ali prejšnje polje
  izbrane vrstice v Pythonovem vrstnem redu in z besedilom `result_column_value`. Nova
  vrstica začne pri prvem polju naprej ali pri zadnjem nazaj. Brez polj se oglasi
  `result_column_unavailable` (SEARCH-M-02).
- Vrstica playlista ima število videov, `playlist_video_count` (SEARCH-04).
- Vrstica rezultata na koncu pove način v vrsti za prenos (`queue_mode_label`), vrstica
  epizode pa `podcast_audio_queued_marker` (PODDL-03). Kot v Pythonu se vrstica
  rezultata, na kateri je fokus, posodobi šele, ko se izbira premakne, seznam epizod pa
  takoj. Oglas ob izbiri za prenos zdaj loči zvok, video in zbirke, kot
  `toggle_download_queue`.
- Ista bližnjica za prenos na istem elementu v 0,35 s se prezre, kot
  `start_download_shortcut` (PODDL-01). Kontekstni meni te zaščite nima, kot v Pythonu.
- Napredek prenosa se v vmesnik sporoči, ko se spremeni cel odstotek ali naslov, sicer
  največ vsakih 0,75 s, kot `make_download_progress_hook` (PODDL-02).

Preverjanje: `cargo build`, `cargo test`, `cargo clippy -D warnings` in `cargo fmt
--check`. Novi testi pokrijejo celotno mapo, odpiranje zbirk iz priljubljenih brez
zamenjave zaporedja, kanal za Odpri kanal, polja vrstice in kroženje po njih, število
videov, oznake vrste za prenos, zaščito bližnjice in omejitev napredka. Samodejni UI
Automation preizkus na ločeni kopiji podatkov je potrdil branje polj v priljubljenih in
odpiranje kanala z Enter in s Ctrl+Shift+O.

### E6: lupina in oglaševanje (28. 9. 2026)

Spremembe:

- Jezikovno okno se pokaže samo ob prvem zagonu brez datoteke nastavitev beta in brez
  Pythonovih nastavitev ter nikoli ob skritem zagonu v pladenj, kot `wx_main.py`
  (SHELL-02, SHELL-03). Kot v Pythonu se nastavitve ob prvem zagonu takoj shranijo, zato
  skriti prvi zagon jezika ne vpraša tudi pozneje. Po izbiri jezika se glavni meni
  oglasi s "Settings saved.", kot `prompt_initial_language`.
- Kadar NVDA besedila ne prevzame, oglas poleg spremembe imena statusne vrstice sproži
  še `EVENT_SYSTEM_ALERT` na glavnem oknu in `EVENT_OBJECT_VALUECHANGE` na statusni
  vrstici, kot `raise_accessibility_alert` (SHELL-M-04). Za JAWS glej dopolnitev spodaj.
- Action Finder ima natanko Pythonov seznam `action_finder_actions` v istem vrstnem
  redu: 16 stalnih postavk, Trending in Resume last session na tretjem mestu, History
  in RSS na koncu ter blok predvajalnika, kadar predvajalnik teče (s Play ali Pause,
  Copy path ali Copy link, YouTube in podcast vstavki). Oznake so Pythonova besedila z
  bližnjico za vejico. Prej je Rust kazal vse registrirane akcije z drugačnimi imeni in
  nikoli akcij predvajalnika. Escape na gumbih Open in Cancel zdaj zapre okno.
- Glavni meni ob vrnitvi izbere postavko zadnjega odprtega zaslona, kot
  `last_activated_menu_action`, tudi kadar je bil zaslon odprt z bližnjico ali iz
  Action Finderja. Osvežitev menija v ozadju (število prenosov, vrsta predvajanja) ohrani
  izbrano postavko po besedilu ali vrstici, kot `refresh_main_menu_download_label`.
  Prej je vsaka osvežitev skočila na prvo postavko.
- Pladenj: ikona je prisotna ves čas seje, kot `setup_taskbar_icon`, in ob obnovitvi okna
  ne izgine. Meni pladnja ima ločilo pred Exit. Ob zapiranju v pladenj se besedilo
  "tray_still_running" zapiše tudi v statusno vrstico, kot `announce_player`.
- Center obvestil: ob odprtju ni več dodatnega oglasa "Notification center: N" ali
  "No notifications.", fokus gre samo na seznam. Tab gre v Pythonovem vrstnem redu
  Back, Play, Clear notifications, seznam. Po brisanju z Delete ostane izbrana ista
  vrstica, torej naslednje obvestilo. Clear notifications ne premakne fokusa.

Dopolnitev po Urhovih odločitvah (28. 9. 2026):

- Oglas, ki ga NVDA ne prevzame, gre zdaj JAWS neposredno s klicem `SayString` na
  tekočem strežniku `FreedomSci.JawsApi`, šele nato na dogodke MSAA. Python ima enak
  namen v `_jaws_speak_ctypes`, vendar kliče `ole32.CoGetActiveObject`, ki ga ole32.dll
  ne izvaža, zato v Pythonu JAWS besedila nikoli ne dobi neposredno. Rust uporablja
  `oleaut32.GetActiveObject` in ob vsakem oglasu vzame svežo referenco. Kadar JAWS
  besedilo prevzame, se dogodki MSAA ne sprožijo, da ga ne prebere dvakrat. Odobreno
  odstopanje O-1.
- Ob vrnitvi iz predvajalnika v center obvestil ali v zgodovino ostane izbrana vrstica,
  iz katere je bil element predvajan. Python izbere prvo vrstico. Odobreno odstopanje O-2.

Preverjanje: `cargo build`, `cargo test`, `cargo clippy --all-targets` in `cargo fmt
--check`. Novi testi pokrijejo pogoje jezikovnega okna in oglas po njem, seznam Action
Finderja z vsemi pogoji in vstavki, izbiro zadnjega zaslona in ohranjanje izbire v
glavnem meniju. Samodejni preizkus na ločeni kopiji podatkov je potrdil, da skriti prvi
zagon ne odpre okna, da viden prvi zagon odpre jezikovno okno samo enkrat, izbiro
Favorites in Notification center ob vrnitvi v meni, vrstni red Tab v centru obvestil,
seznam Action Finderja in Escape z gumba.

### E7: preostali zasloni in GPT-jevo delo za prepis, besedila in poglavja (28. 9. 2026)

Pregled GPT-jevega necommitanega dela (prepis, besedila pesmi, poglavja z zunanjih
povezav) glede na `apricot/media/media.py` in `apricot/ui/misc.py`. Razčlenjevanje SRT in
WebVTT, izbira podnapisov po jezikih, lokalne datoteke ob posnetku, LRCLIB, časovne vrstice
besedil in zunanja poglavja se ujemajo s Pythonom. Popravki:

- Napaka pri prepisu je Pythonov `transcript_failed` z dejanskim besedilom napake. Prej je
  Rust v `{error}` vstavil "No transcript or captions available.". Omejitev YouTube
  (HTTP 429) se pokaže kot `transcript_failed` z besedilom `transcript_rate_limited` in se
  zapomni kot preverjena, zato ponovno odprtje ne poskuša znova, kot v Pythonu.
  Neuspešen HTTP odgovor ima Pythonovo obliko "HTTP Error 403: Forbidden". Privzeti
  User-Agent za podnapise je Pythonov.
- Play, Copy line in Copy timestamp link brez izbrane vrstice oglasijo
  `no_transcript_available`, kot v Pythonu.
- Poglavje brez naslova se imenuje "Chapters" brez številke, kot `normalized_chapters`.
- Prepis, besedila in poglavja izven predvajalnika oglasijo `no_player`, kot
  `ensure_player_for_auxiliary_view`.

Preostali zasloni:

- Podrobnosti (F7) niso več okno. Kot `show_video_details` se pod kontrolami
  predvajalnika pokažejo oznaka, polje samo za branje ter gumba Copy details in Back.
  Fokus gre v polje s kazalko na začetku in oglasi se "Details". Back ali Escape
  (`player_back`) jih skrije, fokus gre na predvajalnik in oglasi se `details_closed`.
  Gumb Back v navigaciji predvajalnika še vedno zapusti predvajalnik. Tab iz polja gre na
  Copy details in Back, nato na začetek strani. Puščice, Home, End, PageUp, PageDown,
  Ctrl+C in Ctrl+A ostanejo v polju (`details_text_navigation_key`), druge bližnjice
  predvajalnika delujejo tudi v polju. Hitrost in višina tona se v polju posodobita
  sproti (`update_details_text`). Z nastavitvijo `show_video_details_by_default` se
  podrobnosti odprejo same ob vsakem novem posnetku (PLAYER2-06).
- Vrsta predvajanja oglasi `playback_queue_removed`, `playback_queue_reordered` in
  `playback_queue_cleared`. Clear queue in odstranitev zadnjega elementa zapreta okno,
  prazna vrsta ob Clear oglasi `playback_queue_empty`. Gumbi Move up, Move down in
  Remove fokusa ne premaknejo več na seznam.
- Naročnine, priljubljene, zgodovina, podcasti in playlisti ob odprtju ne oglasijo več
  števila elementov ("Subscriptions: 3") in ga ne pišejo v statusno vrstico. Python
  statusno vrstico nastavi samo za prazen seznam in ničesar ne oglasi. Po odstranitvi
  naročnine ostane izbrana ista vrstica, torej naslednja naročnina. Open videos in New
  videos brez izbire pokažeta sporočilno okno `no_selection`, kot `self.message`.
- Neposredna povezava brez sheme dobi `https://`, kot `direct_link_item`; prej je Rust
  "youtube.com/watch?v=..." zavrnil kot neveljavno. Prazno polje pokaže sporočilno okno
  `no_selection`.
- Enter v polju za iskanje in v polju neposredne povezave ni naredil ničesar, ker ga je
  `IsDialogMessageW` spremenil v neobdelan ukaz IDOK. Napaka je bila tudi v nameščeni
  beti. Zdaj Enter zažene iskanje oziroma nastavljeno dejanje povezave.
- Po zaprtju vsakega sporočilnega okna se fokus vrne na kontrolo, ki ga je imela prej.
  Prej je ostal na glavnem oknu brez fokusirane kontrole.

Preverjeno brez sprememb: vrstni red Tab na zaslonu neposredne povezave (Back, Play link,
Download link audio, Download link video, Copy direct media URL, polje) in naročnin, meni
naročnin, izbira jezika podnapisov in lokalnih datotek.

Ostaja: prepis in besedila oglašajo prek NVDA in JAWS, ne pa prek MSAA dogodkov glavnega
okna. Vrsta predvajanja spremembe shrani ob zaprtju okna, Python po vsakem koraku; rezultat
je enak. Predloga P-1 in P-2 sta bila odobrena kot O-3 in O-4.

Preverjanje: `cargo build`, `cargo test` (445 uspešnih, 8 izključenih), `cargo clippy
--all-targets -D warnings` in `cargo fmt --check`. Novi testi pokrijejo besedilo napak in
omejitev prepisa, dodajanje `https://` in tipke, ki ostanejo v polju podrobnosti. Samodejni
preizkus na ločeni kopiji podatkov z lokalnim MKA posnetkom s poglavji, SRT in LRC
datotekama je potrdil F7, Tab in Shift+Tab v podrobnostih, sprotno hitrost, Escape,
samodejno odprte podrobnosti, poglavje brez naslova, prepis in besedila iz lokalnih
datotek, brisanje naročnine, Tab in Enter na neposredni povezavi, Enter v iskanju,
fokus po sporočilnem oknu ter premikanje, odstranjevanje in brisanje vrste.

### E8: shuffle, sorodni video, ReplayGain in celozaslonski način (28. 9. 2026)

Spremembe:

- Shift+S (`toggle_shuffle`) preklopi shuffle in oglasi "Shuffle on." ali "Shuffle off.".
  S shuffle Next in samodejno nadaljevanje izbereta naključni drug element trenutnega
  seznama, Previous ostane po vrsti, vrsta predvajanja ima še vedno prednost. Epizode
  podcasta in playlista gredo po vrsti, dokler naslednja obstaja, kot v
  `relative_player_item`. Izbira elementa s seznama shuffle izklopi (`play_selected`).
- Ctrl+Shift+G (`cycle_replaygain_mode`) zamenja Off, Track, Album, nastavitev takoj
  shrani, jo pošlje mpv (`replaygain`) in oglasi "Audio normalization: Track.". Če ukaza
  ni mogoče poslati, sledi še "Audio normalization will apply when playback restarts.".
  Gumb v predvajalniku se imenuje po trenutnem načinu ("Audio normalization: Off
  Ctrl+Shift+G"), kot `audio_normalization_status_label`. Nov posnetek v isti libmpv
  instanci dobi nastavljeni način.
- Ctrl+Shift+PageDown (`play_related_item`) v ozadju prenese stran YouTube videa, iz
  `ytInitialData` prebere sorodne videe (`lockupViewModel` in `compactVideoRenderer`,
  enako kot Python), preskoči že predvajane v tej seji, z njimi zamenja rezultate iskanja
  in predvaja prvega. Next nato gre po sorodnih videih. Brez YouTube posnetka se oglasi
  "No related video available.", brez predvajalnika "No player.". Statusna vrstica
  med nalaganjem pokaže "Loading related video..." brez oglasa.
- Ob koncu posnetka z vklopljenima "autoplay next" in "autoplay related" se namesto
  naslednjega elementa predvaja sorodni video (`handle_player_eof`). Če ga ni, se
  predvaja naslednji element, brez njega pa se oglasi "Playback finished." (z
  nastavitvijo `announce_playback_finished`), kot `play_next_standard_fallback`. Enako
  zdaj velja za konec z "autoplay next" brez naslednjega elementa, kjer je Rust prej
  oglasil "No next item.". Glej predlog O-6.
- F11, postavka menija in potrditveno polje Full screen (`toggle_player_fullscreen`)
  razširijo okno čez cel zaslon brez okvirja in oglasijo "Full screen on." ali "Full
  screen off.". F11 in meni premakneta fokus na predvajalnik, potrditveno polje obdrži
  fokus. Escape v celozaslonskem načinu najprej zapre podrobnosti, nato izklopi cel
  zaslon brez oglasa in fokus postavi na predvajalnik (`exit_fullscreen_to_player`),
  šele naslednji Escape zapusti predvajalnik. Nastavitev `player_fullscreen` odpre vsak
  nov predvajalnik čez cel zaslon. Ob odhodu s strani predvajalnika se okno vrne v
  prejšnjo velikost. mpv lastnosti `fullscreen` Rust ne nastavlja, ker je video vgrajen
  v okno (`wid`) in celozaslonsko je glavno okno, kot pri Pythonovem `ShowFullScreen`.

Predloga O-5 in O-6 (glej razdelek 5) sta bila odobrena 28. 9. 2026. Med delom sem opazil, da
Rust nima predvajanja v ozadju (PLAYER2-M-10), zato celozaslonska različica gumba "Back to
results" ni narejena. Predvajanje v ozadju je zdaj enota E13a.

Preverjanje: `cargo build`, `cargo test` (452 uspešnih, 8 izključenih), `cargo clippy
--all-targets -D warnings` in `cargo fmt --check`. Novi testi pokrijejo branje sorodnih
videov iz strani, zavrnitev naslovov zunaj youtube.com, preskok že videnih videov in
zamenjavo rezultatov, naključni Next s shuffle, cikel ReplayGain s shranjevanjem in ime
gumba. Izključen omrežni test je prebral sorodne videe z dejanske YouTube strani.
Samodejni preizkus nameščene bete na ločeni kopiji podatkov je potrdil: ime gumba in
oglas pri Ctrl+Shift+G ter shranjeni način, Shift+S, F11 (okno 1920 x 1080 brez okvirja,
fokus na predvajalniku, potrditveno polje obkljukano), Escape (okno nazaj, fokus na
predvajalniku, brez oglasa), preslednico na potrditvenem polju (fokus ostane), dva Escapa
za izhod, Ctrl+Shift+PageDown na YouTube videu (predvaja sorodni video) in Ctrl+PageDown
za naslednji sorodni video.

### E9: izbira izhodne naprave (28. 9. 2026)

Spremembe:

- O (`player_output_devices`), gumb "Audio output devices" in postavka kontekstnega
  menija odprejo izbirno okno "Audio output devices" s pozivom "Select audio output
  device" in seznamom mpv `audio-device-list` tekočega predvajalnika, prvi element je
  izbran. Oznaka je "opis (ime)", kot v `show_output_devices`. Izbira preklopi napravo
  takoj (mpv `audio-device`), velja do konca seje predvajalnika in oglasi "Audio output
  device set to ...". Privzeta naprava v nastavitvah se ne spremeni. Escape zapre okno
  brez oglasa, fokus se vrne na kontrolo predvajalnika. Brez predvajalnika O ne naredi
  ničesar, prazen seznam oglasi "No audio output devices were found.".
- Rust seznam naprav dobi z opazovanjem lastnosti `audio-device-list`, zato se ob
  priključitvi ali odklopu naprave osveži sam in O ne čaka na mpv.
- Nastavitve, razdelek Playback: polje "Default audio output device" je prej imelo samo
  "auto", zato bi shranjevanje nastavitev tiho izbrisalo shranjeno napravo. Zdaj ima
  "auto" in shranjeno napravo, ob odprtju razdelka pa se v ozadju zažene preizkus
  naprav (`refresh_audio_output_devices_async`). Ko konča, se seznam zamenja na mestu,
  izbrana vrednost ostane in fokus se ne premakne. Preizkus je kratko živeč libmpv
  odjemalec brez okna, ki nadomesti Pythonov `mpv --audio-device=help`. Rezultat se
  hrani 20 sekund za ponovno odprtje in 60 sekund do naslednjega preizkusa, kot v
  Pythonu. Shranjena naprava, ki je preizkus ne najde, ima oznako "ime (No audio output
  devices were found.)".
- Varen nadomestek (`check_saved_audio_device_available`): 6,5 sekunde po vidnem zagonu
  (ne ob zagonu v sistemsko vrstico) Rust preveri shranjeno napravo, če ni "auto". Če je
  ni več, pokaže opozorilo "The saved audio output device is no longer available.
  Choose a new default device." in nato izbirno okno "Default audio output device" z
  izbranim "auto". OK shrani izbiro in oglasi "Settings saved.", Cancel shrani "auto"
  brez oglasa. Če je takrat odprto drugo modalno okno, preverjanje počaka nanj.
- Popravek izbirnih oken in oken za ime (`playlist_dialog_win32`): Enter v seznamu ali
  polju ni naredil ničesar, ker ga je `IsDialogMessageW` spremenil v ukaz `IDOK`, ki ga
  okno ni poznalo. To je veljalo za vse izbire s tem oknom (playlisti, hitrost podcasta,
  kategorije, format prenosa in druge). Zdaj Enter potrdi izbiro, Escape jo prekliče.
  Enako napako iz kode sumim tudi v oknu zaznamkov in v oknu za izbiro jezika ob prvem
  zagonu. Nisem je preveril v živo, zato ju nisem spreminjal.

Odstopanji, ki ostajata: če mpv zavrne ukaz za preklop naprave šele v predvajalni niti,
Rust oglasi splošno "Timing is not available yet." namesto `stream_url_failed`, ker se
ukazi izvajajo asinhrono. EQ profil za posamezno napravo se po preklopu še ne uporabi,
ker izenačevalnik predvajalnika še ni narejen (E10).

Preverjanje: `cargo build`, `cargo test` (457 uspešnih, 10 izključenih), `cargo clippy
--all-targets -D warnings` in `cargo fmt --check`. Novi testi pokrijejo branje
`audio-device-list`, oznake v izbirnem oknu, začetni seznam v nastavitvah s shranjeno
napravo, seznam po preizkusu z manjkajočo napravo, zaznavo manjkajoče naprave in
ohranitev izbrane vrednosti ob osvežitvi. Izključen test z dejansko libmpv je potrdil
preizkus naprav ("auto" je prvi). Samodejni preizkus nameščene bete na ločeni kopiji
podatkov je potrdil: O odpre okno s fokusom na "Autoselect device (auto)" in devetimi
napravami, Escape vrne fokus na predvajalnik brez oglasa, puščica dol in Enter
preklopita napravo in oglasita "Audio output device set to Line 1 (Virtual Audio Cable)
(...)", privzeta naprava ostane "auto". Ob zagonu z neobstoječo shranjeno napravo se po
6,5 sekunde pokaže opozorilo, nato izbira z "auto", Enter shrani "auto" in oglasi
"Settings saved.". V nastavitvah se seznam naprav po preizkusu osveži, izbrana ostane
shranjena naprava in fokus ostane na seznamu razdelkov.

### E10: izenačevalnik in EQ profili (28. 9. 2026)

Spremembe:

- F4, gumb "Equalizer" in postavka kontekstnega menija odprejo okno "Equalizer" kot
  Pythonov `show_player_equalizer`. Vrstni red Tab: "Equalizer preset" (izbran je
  učinkoviti preset, tudi preset izhodne naprave), "Equalizer range in dB", "Custom
  preset name" (samo pri lastnih profilih), deset drsnikov "Equalizer 31 Hz sub bass
  rumble" do "Equalizer 16 kHz air and sparkle", nato gumbi v Pythonovem vrstnem redu
  ustvarjanja: OK, Cancel, Reset this preset, Save as global equalizer preset, Add
  equalizer profile, Delete equalizer profile (onemogočen pri tovarniških presetih),
  Import, Export, Compare with original, Save as default for this output device, Clear
  output device default. Drsnik ima ime oznake, vrednost "3.0 dB" in opis "oznaka:
  vrednost" kot `SliderAccessible`; puščice premaknejo za 1 dB, Page Up in Page Down za
  3 dB. Sprememba se sliši po 160 ms. Enter shrani obseg in ime ter oglasi "Equalizer
  saved.", izenačevalnik ostane za to sejo predvajalnika. Escape, Cancel ali zaprtje
  okna vrnejo prejšnji izenačevalnik in oglasijo "Equalizer closed.". Fokus se vrne na
  kontrolo predvajalnika. Brez predvajalnika F4 ne naredi ničesar.
- Stanje izenačevalnika je zdaj kot v Pythonu: seja predvajalnika ima lasten
  izenačevalnik samo po F4 (`session_equalizer_*`), sicer sledi nastavitvam v živo, s
  presetom izhodne naprave pred globalnim presetom. Prej je Rust ob začetku seje
  prepisal `global_equalizer_gains` in ni upošteval ne preseta ne naprave. Bass boost pri
  izklopljenem izenačevalniku doda svojo krivuljo ravnemu odzivu, kot v Pythonu (prej je
  prištel shranjene vrednosti). Zaščita pred popačenjem velja samo, ko kak pas ojača.
- Filter v mpv: izenačevalnik se zamenja z `af add`/`af remove` z izmeničnima oznakama
  `@apricot_eq` in `@apricot_eq_next`, višina tona z `af-command apricot_pitch set-pitch`
  oziroma `af add`/`af remove` kot v Pythonu. Prej je vsaka sprememba izenačevalnika,
  bass boosta ali višine tona na novo nastavila celoten niz filtrov (`set af`).
  Po preklopu izhodne naprave z O se uporabi preset te naprave, če seja nima lastnega
  izenačevalnika.
- Nastavitve, razdelek Equalizer: oznake pasov so Pythonove ("Equalizer 1 kHz midrange
  presence" namesto "Equalizer 1000 Hz"), drsniki imajo ime, vrednost in opis kot v
  Pythonu namesto vrednosti v imenu, tovarniški preseti kažejo svoje tovarniške vrednosti.
  Gumbi Reset this preset, Add, Import, Export in Delete equalizer profile delujejo kot v
  Pythonu (vnos imena, datoteka JSON v Pythonovi obliki, potrditev brisanja, fokus po
  dodajanju, uvozu in brisanju na "Equalizer preset"). Sprememba imena lastnega profila
  takoj posodobi seznam presetov. Ob odprtem predvajalniku se spremembe slišijo kot v
  Pythonu (drsnik, preset, vklop, zaščita, preset naprave, ponastavitev).
- Popravek Enter in Escape v oknu zaznamkov (Enter predvaja izbrani zaznamek) in v izbiri
  jezika ob prvem zagonu (Enter potrdi jezik). Oba sta imela isto napako kot izbirna okna
  v E9.

Odstopanja, ki ostajajo: Python ob neuspehu `af add` poskusi še dvakrat po 180 ms, Rust
napako sporoči takoj (libmpv ukaz je sinhron). Python po vklopu ali izklopu volume
boosta ponovno uporabi isti izenačevalnik, Rust tega ne naredi, ker se zvok ne spremeni.
Okno zaznamkov ima Enter, Delete in Escape fiksne, Python pa uporablja nastavljive
bližnjice `open_selected`, `remove_selected` in `player_back`. Gumbi okna izenačevalnika
so vizualno v mreži, vrstni red Tab je Pythonov.

Preverjanje: `cargo build`, `cargo test` (469 uspešnih, 11 izključenih), `cargo clippy
--all-targets -D warnings` in `cargo fmt --check`. Novi testi pokrijejo menjavo oznak
izenačevalnika, `af-command` za višino tona s ponovnim dodajanjem, učinkovito stanje s
presetom naprave, sejo in bass boostom, ustvarjanje, poimenovanje in brisanje profilov,
izvoz in uvoz v Pythonovi obliki, oznake pasov ter vrstni red gumbov. Nov izključen test
z dejansko libmpv med predvajanjem zamenja izenačevalnik dvakrat, doda, spremeni in
odstrani filter višine tona ter potrdi, da predvajanje teče naprej. Samodejni preizkus
nameščene bete na ločeni kopiji podatkov je potrdil vrstni red Tab, imena in vrednosti
drsnikov prek MSAA, puščico desno (5.0 na 6.0 dB), Page Up (3.0 dB), Enter z oglasom
"Equalizer saved.", ohranjen izenačevalnik ob ponovnem F4, Escape z "Equalizer closed."
in neshranjenim obsegom, v nastavitvah vklop, dodajanje profila "Live" z imenom in
vrednostmi v `settings.json` ter brisanje s potrditvijo, Enter v zaznamkih in v izbiri
jezika ob prvem zagonu.

### E11: BPM analiza in drugi Enter v iskanju (28. 9. 2026)

Spremembe:

- B v predvajalniku oglasi "Analyzing tempo..." in nato "{bpm} BPM." ali "BPM not
  available." kot Pythonov `announce_bpm_async`. Brez odprtega predvajalnika takoj oglasi
  "BPM not available.". Ponovni B med analizo istega posnetka pri isti hitrosti in višini
  tona oglasi samo "Analyzing tempo..." in ne zažene nove analize. ffmpeg dekodira do 72
  sekund od 18 sekund pred trenutnim položajem z enakim filtrom kot Python (celoten pas in
  pas 35 do 250 Hz, 11025 Hz, s16le), največ 35 sekund. Rezultat je tempo izvirnika,
  pomnožen s hitrostjo predvajanja in zaokrožen kot v Pythonu. Če se med analizo
  zamenja posnetek, hitrost ali višina tona ali se predvajalnik zapre, se ffmpeg ustavi
  in rezultat se ne oglasi. ffmpeg se išče kot Pythonov `ffmpeg_executable`: nastavljena
  datoteka ali mapa, priložena kopija, nato `PATH`.
- Ocena tempa (`apricot-media` `tempo.rs`) je prenos Pythonovega `apricot/media/tempo.py`
  vrstico za vrstico, vključno z Pythonovim zaokroževanjem polovic na sodo število. Na
  šestih posnetkih (trije ritmi, ton, govor, testni MKA), dekodiranih z Pythonovimi
  argumenti ffmpeg, sta Python in Rust vrnila enak tempo na šest decimalk oziroma oba
  "ni tempa".
- Hrošč v iskanju, ki ga je opisal Urh: drugi Enter v iskalnem polju, preden so prišli
  rezultati prvega, je pustil iskanje brez rezultatov. Drugo iskanje je dobilo novo
  generacijo, komponenta YouTube pa ga je zavrnila z angleškim "a YouTube search is already
  active", zato so bili rezultati prvega zavrženi kot zastareli. Zdaj novo iskanje najprej
  prekliče tekoče delo, kot Python začne novo generacijo, in rezultati se odprejo (v
  preizkusu po 7 sekundah, ker komponenta najprej konča prvo iskanje). Enter v praznem
  polju zdaj odpre sporočilno okno "Enter a search query." kot Python, prej je statusna
  vrstica oglasila angleško "search query is empty". Osnovni Enter v iskalnem polju je
  bil popravljen že v E7 (`IsDialogMessageW`), ne v E3.

Odstopanja, ki ostajajo: Rust za tokove ne hrani dodatnih HTTP glav, zato jih ffmpeg ne
dobi (predvajalnik jih prav tako ne uporablja). Enako kot v Pythonu BPM ni na voljo, če
predvajalnik dobi video tok brez zvoka in ločen zvočni tok. Tudi odpiranje kanala ali
seznama predvajanja med tekočim iskanjem verjetno naleti na isto zavrnitev komponente;
to ni preverjeno in ostaja za E14.

Preverjanje: `cargo build`, `cargo test` (479 uspešnih, 12 izključenih), `cargo clippy
--all-targets -D warnings` in `cargo fmt --check`. Novi testi pokrijejo Pythonovo
zaokroževanje, mediano in percentil, tempo ritmov 90 do 140 BPM, tišino in prekratek
posnetek, ključ stanja, okno analize, tempo pri hitrosti, argumente ffmpeg, iskanje
ffmpeg in zamenjavo tekočega iskanja po preklicu. Nov izključen test z dejanskim ffmpeg
najde 60 BPM v piskih na sekundo. Samodejni preizkus nameščene bete (skrita kopija brez
NVDA odjemalca, tipke poslane kot sporočila oknu) je potrdil B z "Analyzing tempo...",
ponovni B med analizo, "137 BPM." za lokalni ritem, "138 BPM." po D (1,01x), brez oglasa
po D med analizo, ter v iskanju dvojni Enter in tri zaporedna iskanja z Escape vmes.
