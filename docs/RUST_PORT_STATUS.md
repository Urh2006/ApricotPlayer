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
| PLAYER2-06 | P2 | `show_video_details_by_default` samo fokusira gumb Details; Python samodejno odpre podrobnosti. |
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
| PLAYER2-M-03 | P1 | Izbira izhodne naprave (O) | `apricot/player/volume.py`, `apricot/ui/player.py` |
| PLAYER2-M-05 | P1 | Preklop shuffle (Shift+S), sorodni video (Ctrl+Shift+PageDown), ReplayGain (Ctrl+Shift+G), celozaslonski način (F11) | `apricot/ui/misc.py:1358`, `apricot/ui/player.py` |
| PLAYER2-M-01 | P1 | BPM analiza (B) | `apricot/ui/misc.py:2710-2803` |
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

Nepregledano ali le delno pregledano. Ti deli se primerjajo na začetku ustrezne enote:
Action Finder, pladenj in zapiranje v pladenj, center obvestil, zaslon naročnin,
zaslon vrste predvajanja, dialog neposredne povezave, podrobnosti (F7), poglavja,
obnovitev fokusa ob vrnitvi v glavni meni ter natančna besedila napak pri iskanju.

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
- **E7. Revizija in popravki preostalih zaslonov.** Naročnine, vrsta predvajanja,
  neposredna povezava, podrobnosti in poglavja ter trenutno necommitano delo za
  lyrics in transcript. Python podrobnosti (F7) niso dialog, ampak vgrajeno polje
  za branje z gumboma Copy details in Back na zaslonu predvajalnika. Rust jih ima
  kot dialog, zato sem vanjo prestavil tudi PLAYER2-06 (samodejno odpiranje
  podrobnosti) in sprotno posodabljanje hitrosti in višine tona v podrobnostih.

### B. Majhne manjkajoče funkcije predvajalnika

- **E8.** Shuffle, sorodni video, ReplayGain cikel in celozaslonski način.
- **E9.** Izbira izhodne naprave (O) z osvežitvijo seznama in varnim nadomestkom.
- **E10.** Izenačevalnik iz predvajalnika (F4) ter EQ profili v nastavitvah.
- **E11.** BPM analiza.
- **E12.** Način urejanja z varnim shranjevanjem kopije in zamenjavo izvirnika.
- **E13.** Komentarji.

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
