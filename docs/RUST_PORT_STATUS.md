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
| SEARCH-01 | P1 | Odpravljeno v E14 in E15: ponudnik SoundCloud s tipi Track, Playlist in User (playlisti in uporabniki po O-11). |
| SEARCH-08 | P2 | Odpravljeno v E14: gumbi Play, Download audio, Download video in Add favorite ter rezultati na iskalnem zaslonu. |
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
| PLAYER2-M-10 | P2 | Odpravljeno v E13a: predvajanje v ozadju | `apricot/ui/player.py:648-812`, `apricot/ui/events.py:605-620` |
| PLAYER2-M-01 | P1 | Odpravljeno v E11: BPM analiza (B) | `apricot/ui/misc.py:2710-2803` |
| PLAYER2-M-04 | P1 | Odpravljeno v E13: komentarji (Ctrl+Shift+M) | `apricot/ui/misc.py:2095+` |
| PLAYER2-M-06 | P1 | Odpravljeno v E12: način urejanja (E, Ctrl+S, Ctrl+R) | `apricot/ui/misc.py:2389-2433` |
| SEARCH-M-01 | P1 | Odpravljeno v E14 in E15: SoundCloud iskanje, izvajalci, seti ter iskanje playlistov in uporabnikov | `apricot/search/search.py:451-474, 703-729` |
| SEARCH-M-02 | P1 | Odpri kanal (Ctrl+Shift+O) in stolpci rezultatov (Ctrl+Alt+levo/desno) | `apricot/search/search.py:233` |
| SEARCH-M-04 | P2 | Odpravljeno v E14: vmešavanje Shorts v iskanje in v objave kanala | `apricot/search/search.py:374-495` |
| SHELL-M-03 | P1 | Odpravljeno v E15: kopiranje diagnostičnega poročila | `apricot/system/diagnostics.py` |
| SETTINGS-M-01 | P1 | Piškotki: datoteka, uvoz iz brskalnika, DevTools izvoz, prijavni profil | `apricot/ui/cookies.py` |
| SETTINGS-M-03 | P2 | Set default player in pomoč pri povezavah datotek | `apricot/ui/settings.py:462` |
| PODDL-M-01 | P1 | Pretvornik datotek in pretvornik map | `apricot/media/media.py:40-436` |
| PODDL-M-03, SHELL-M-02 | P1 | Odpravljeno v E18: yt-dlp posodobitve ter posodobitve aplikacije s kanali, preskokom verzije, preverjanjem in rollbackom | `apricot/updater/updater.py` |
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
- **E12. Način urejanja z varnim shranjevanjem kopije in zamenjavo izvirnika. Zaključeno 28. 9. 2026, glej razdelek 6.**
- **E13. Komentarji. Zaključeno 28. 9. 2026, glej razdelek 6.**
- **E13a. Predvajanje v ozadju. Zaključeno 29. 9. 2026, glej razdelek 6.**

### C. Večji manjkajoči sklopi

- **E14.** SoundCloud, Shorts vmešavanje in gumbi na iskalnem zaslonu.
- **E15.** Diagnostično poročilo z zakrivanjem zasebnih podatkov.
- **E16. Piškotki. Zaključeno 29. 9. 2026, glej razdelek 6.**
- **E17. Pretvornik datotek in pretvornik map. Zaključeno 29. 9. 2026, glej razdelek 6.**
- **E18. yt-dlp posodobitve in posodobitve aplikacije. Zaključeno 29. 9. 2026, glej
  razdelek 6.** Namestitev posodobitve aplikacije ostane po D-011 v lokalni beti
  onemogočena.
- **E19. AudioVault. Zaključeno 30. 9. 2026, glej razdelek 6.**
- **E20. Zaključna parity vrata. Strojni del zaključen 30. 9. 2026, glej razdelek 6.**
  Odprti ostajajo ročni NVDA preizkus celote, pravi računi, dolgi teki in izdaja 2.0.

## 5. Odobrene odločitve (27. 9. 2026)

1. Polje YouTube backend se odstrani iz nastavitev, vedno se uporablja yt-dlp.
   Koda pomožnega programa ostane v repozitoriju neuporabljena.
2. Ključi, ki jih je GPT dodal v Pythonov `en.json`, se premaknejo na Rust stran,
   `en.json` pa se vrne v stanje iz `main`.
3. Rust beta ostane v ločenem imeniku z enkratnim uvozom Python podatkov.
4. (30. 9. 2026) ApricotPlayer 2.0 nadomesti Python verzijo in ne teče ob njej; ob objavi
   izide s Pythonovimi imeni paketov. Glej razdelek 6, "Rust 2.0 nadomesti Python".

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
- **O-7.** Shranjevanje v načinu urejanja pri vklopljenem, a ploskem izenačevalniku (vsi
  pasovi na 0, na primer preset Flat): Python v mpv poda prazen filter `lavfi=[]`, ki ga
  mpv zavrne, zato shranjevanje vedno spodleti. Rust ploskega izenačevalnika ne doda, kot
  ga ne doda niti predvajalnik, zato shranjevanje uspe (predlog P-3 iz E12, odobren
  28. 9. 2026).
- **O-8.** Kadar shranjevanje kopije (Ctrl+S) spodleti med pisanjem, Python pusti delno
  datoteko "ime - edited". Rust jo izbriše, kot že pri zamenjavi izvirnika izbriše
  začasno datoteko (predlog P-4 iz E12, odobren 28. 9. 2026, narejeno v E13).
- **O-9.** Kadar se predvajanje začne s priljubljenih, zgodovine, podcasta ali seznama
  predvajanja, Python v vgrajenem seznamu predvajalnika pokaže stare rezultate zadnjega
  iskanja. Rust vgrajenega seznama takrat ne pokaže (predlog P-5 iz E13a, odobren
  29. 9. 2026).
- **O-10.** Brez predvajanja v ozadju Python ob odprtju nastavitev s strani predvajalnika
  ustavi predvajanje. Rust nastavitve odpre kot okno nad predvajalnikom in predvajanje
  pusti teči (predlog P-6 iz E13a, odobren 29. 9. 2026).
- **O-11.** SoundCloud iskanje playlistov in uporabnikov: Python kliče spletni API
  SoundClouda prek vgrajenega `yt-dlp`, ki javni ID spletnega odjemalca pobere s strani
  soundcloud.com. Samostojni `yt-dlp.exe` te poti nima, zato Rust ID pobere sam na enak
  način (skripte s soundcloud.com, vzorec `client_id:"..."`, ponovno branje ob odgovoru
  401 ali 403) in ga hrani v pomnilniku do konca zagona (odločitev P-7, odobrena
  29. 9. 2026, narejeno v E15).
- **O-12.** Kadar pretvorba ene datoteke spodleti, Python pusti delno izhodno datoteko
  oziroma pri zamenjavi izvirnika skrito datoteko ".ime.apricot-converting-...". Rust jo
  izbriše, kot Python že naredi pri pretvorbi mape z zamenjavo (predlog P-8 iz E17,
  odobren 29. 9. 2026, narejeno pred E18).
- **O-13.** Ko uporabnik v dialogu za shranjevanje pretvorbe potrdi prepis obstoječe
  datoteke, Python vseeno zapiše "ime (2).končnica". Rust prepiše izbrano datoteko. Pretvorba
  gre prek skrite delovne datoteke, zato obstoječa datoteka ob napaki ostane nespremenjena.
  Izvirnika samega se tudi s potrditvijo ne prepiše, takrat ostane "ime (2)" (predlog P-9 iz
  E17, odobren 29. 9. 2026, narejeno pred E18).
- **O-14.** Python ob novem AudioVault zaslonu obdrži rezultate prejšnjega zaslona, zato Play
  ali Download audio na vrstici "No search results." predvaja ali prenese prvi element starega
  seznama. Rust ob novem zaslonu rezultate počisti (predlog P-10 iz E19, odobren 30. 9. 2026,
  narejeno v E19).

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

### E12: način urejanja (28. 9. 2026)

Spremembe:

- E in gumb "Edit mode" preklopita način urejanja kot
  Pythonov `toggle_edit_mode`: "Edit mode on." ali "Edit mode off.". Pri posnetku, ki ni
  obstoječa lokalna datoteka, se oglasi "Edit mode is available only for local files.",
  brez predvajalnika se ne zgodi nič. Način se izklopi ob vsakem novem posnetku, kot v
  Pythonovem `start_mpv`. Prej je E oglasil, da funkcija v beti še ni na voljo.
- Ctrl+S (`player_save_edit_copy`) pri vklopljenem načinu oglasi "Saving edited file." in
  v ozadju shrani kopijo "ime - edited.končnica" (ali "ime - edited (2).končnica" in
  naprej) ob izvirniku, nato oglasi "Edited file saved: ...". Pri izklopljenem načinu
  Ctrl+S in Ctrl+R ne naredita nič, kot v Pythonu.
- Ctrl+R (`player_replace_edit_original`) oglasi "Saving edited file.", tiho ustavi
  predvajalnik (stran predvajalnika ostane odprta, fokus ostane na mestu), zapiše novo
  datoteko v skrito sosednjo začasno datoteko ".ime.apricot-converting-xxxxxxxxxxxx.končnica"
  in šele po uspehu z njo zamenja izvirnik ter oglasi "Original file replaced: ...".
- Pot je Pythonova: mpv (`mpv.exe` iz namestitve) predvaja datoteko brez zvoka v začasen
  WAV z enako verigo kot predvajalnik (hitrost, filter za hitrost glede na
  `speed_audio_mode`, višina tona glede na `pitch_mode`, izenačevalnik z bass boostom),
  ffmpeg pa ga zapiše v izhodno datoteko s Pythonovimi kodeki (MP3 320k, Opus 160k, WAV,
  FLAC, sicer AAC 256k). Pri videu ffmpeg ohrani sliko (`-c:v copy`) ali jo pri spremenjeni
  hitrosti pospeši z `setpts` in libx264. Brez mpv se uporabi samo ffmpeg z Pythonovim
  `local_edit_ffmpeg_args` (izenačevalnik, Rubberband ali veriga `atempo`).
- Napaka odpre sporočilno okno "Could not save edited file: ..." z zadnjimi 600 znaki
  izpisa programa ali "... exited with code N", kot Python. Začasna datoteka za zamenjavo
  se ob napaki izbriše, izvirnik ostane nespremenjen.

Predlog P-3 (odobren kot O-7): Python med vklopljenim izenačevalnikom vedno doda filter
izenačevalnika, tudi ko so vsi pasovi na 0 (na primer preset Flat). mpv prazen graf
`lavfi=[]` zavrne ("Creating filter 'lavfi' failed"), zato Pythonovo shranjevanje v tem
primeru vedno javi napako. To sem preveril z dejanskim `mpv.exe`. Rust ploskega
izenačevalnika ne doda, kot ga ne doda niti predvajalnik, zato shranjevanje uspe.

Predlog P-4 (odobren kot O-8, narejen v E13): kadar shranjevanje kopije (Ctrl+S) spodleti med pisanjem, Python
pusti delno datoteko "ime - edited". Rust se zdaj obnaša enako; predlagam, da Rust delno
kopijo izbriše, kot to že naredi pri zamenjavi izvirnika.

Odstopanja, ki ostajajo: besedilo sistemske napake je Windowsovo ("Access is denied. (os
error 5)"), Python pa ga oblikuje kot "[WinError 5] Access is denied: ...". Po Ctrl+R se
naslov okna vrne na "ApricotPlayer 2 Beta", kot pri vsakem ustavljenem predvajalniku v
Rustu. Python ob Ctrl+R uniči ploščo predvajalnika, na kateri je fokus, Rust fokus pusti
na mestu.

Preverjanje: `cargo build`, `cargo test` (491 uspešnih, 12 izključenih), `cargo clippy
--all-targets -D warnings` in `cargo fmt --check`. Novi testi pokrijejo verigo `atempo`,
argumente mpv za Rubberband, mpv višino tona in ploski izenačevalnik, filtre in argumente
samega ffmpeg, korak združevanja za zvok in video, kodeke, video končnice, imena izhodnih in
začasnih datotek, izklop načina ob novem posnetku, napako brez kodirnika z brisanjem začasne
datoteke in besedilo napak. Samodejni preizkus nameščene bete (skrita kopija brez NVDA
odjemalca, tipke poslane oknu, Ctrl samo v stanju tipkovnice bete) na ločeni kopiji podatkov
z začetno hitrostjo 1,5x in vklopljenim ploskim izenačevalnikom je potrdil: Ctrl+S brez
načina ne naredi nič, E, E, E oglasijo on, off, on, Ctrl+S ustvari "song - edited.mka" z
dolžino 26,8 s namesto 40 s, Ctrl+R zamenja "song.mka" (26,8 s) brez ostankov začasnih
datotek, fokus ostane na predvajalniku, E po zamenjavi ne naredi nič. Video MP4 pri 2x je dal
6,1 s namesto 12 s s sliko in zvokom. Zamenjava datoteke samo za branje je odprla okno
"Could not save edited file: Access is denied. (os error 5)", izvirnik je ostal cel. Ročno
sem z dejanskim mpv in ffmpeg preveril še verigo Rubberband za hitrost in višino tona z
zaščito izenačevalnika pred popačenjem.

### E13: komentarji in odobritev P-3 in P-4 (28. 9. 2026)

Spremembe:

- Predloga P-3 in P-4 iz E12 sta odobrena kot O-7 in O-8. Za O-8 Rust ob neuspelem
  Ctrl+S izbriše delno datoteko "ime - edited" (nov test).
- Ctrl+Shift+M, gumb "Comments" in postavka "Comments" v kontekstnem meniju predvajalnika
  odprejo okno komentarjev kot Pythonov `show_comments`. Prej so oglasili, da funkcija v
  beti še ni na voljo. Brez predvajalnika se oglasi "Player not found.", pri posnetku
  brez YouTube ID (lokalna datoteka) "Comments are disabled or unavailable for this
  video.".
- Okno "Comments" ima Pythonov vrstni red Tab: iskalno polje "Search comments", izbirni
  seznam "Sort comments" (Original order, Newest first, Oldest first, Most liked, Most
  replies), seznam "Comments" ter gumbe "Open comment", "Copy comment", "Copy visible
  comments", "Open author channel", "Load more comments" in "Back to main menu". Fokus je
  najprej v iskalnem polju, seznam kaže "Loading comments...", vsi gumbi razen Back so
  onemogočeni.
- Nalaganje teče v ozadju kot Pythonov `fetch_comments_worker`: z nastavljenim ključem
  YouTube Data API stran 20 niti v vrstnem redu relevance, sicer ali po napaki prve strani
  yt-dlp z največ 20 komentarji. Oglas je "Loaded N comments from YouTube Data API." ali
  "... from yt-dlp.", brez komentarjev "Comments are disabled ...", ob napaki "Could not
  load comments: ..." s Pythonovimi namigi za piškotke in prijavo. Po nalaganju naslednje
  strani je izbran prvi nov komentar.
- Vrstice seznama, besedilo za kopiranje, podrobnosti komentarja, iskanje (tudi po
  odgovorih), stabilno razvrščanje in zaokroževanje števil ("4.8M likes") so Pythonovi.
  Enter (`open_selected`) na seznamu odpre okno "Comment details" z besedilom samo za
  branje in gumbom Back, Escape ga zapre in fokus se vrne na seznam. `player_back` na
  seznamu in Escape kjerkoli zapreta okno komentarjev, fokus se vrne v predvajalnik.
- Kontekstni meni seznama (tipka Applications, Shift+F10, desni klik) ima Pythonove
  postavke: Open comment, Copy comment, Copy visible comments, Open author channel,
  ločilo, Load more comments, z enakim omogočanjem.
- Kadar se onemogoči gumb s fokusom (na primer "Load more comments" med nalaganjem), se
  fokus premakne na naslednjo kontrolo, kot to naredi wxWidgets.

Odstopanja, ki ostajajo: HTTP napake API so oblikovane kot v Pythonu ("HTTP Error 403:
Forbidden"), omrežne napake pa imajo besedilo knjižnice reqwest namesto urllib. yt-dlp
dobi piškotke iz nastavitev takoj, Python pa jih doda šele ob ponovnem poskusu po napaki
prijave.

Opažanje zunaj te enote: podrobnosti predvajalnika (F7) število ogledov krajšajo z
odrezovanjem (1999 je "1.9K"), Python pa zaokroži ("2.0K"). Komentarji uporabljajo
Pythonovo zaokroževanje. Predlagam popravek v eni od naslednjih enot.

Preverjanje: `cargo build`, `cargo test` (503 uspešni, 12 izključenih), `cargo clippy
--all-targets -D warnings` in `cargo fmt --check`. Novi testi pokrijejo normalizacijo
niti API in komentarjev yt-dlp (meja 20, naslovi kanalov avtorjev), vrstice seznama,
besedilo za kopiranje in podrobnosti, iskanje in razvrščanje, stanje okna med
nalaganjem, napako in praznim rezultatom, izbiro vira (API, yt-dlp in združena napaka),
izvorni naslov, argumente yt-dlp in brisanje delne kopije. Z dejanskim `yt-dlp.exe` sem
preveril, da argumenti vrnejo 20 komentarjev v približno 5 s. Samodejni preizkus kopije
bete z ločenimi podatki (tipke poslane oknu) na videu "Me at the zoo": okno "Comments" s
fokusom v iskalnem polju in "Loading comments...", po 4 do 6 s 20 komentarjev z
omogočenimi gumbi razen "Load more comments", Tab na "Sort comments", puščica dol
razvrsti "Newest first", Tab na seznam, Enter odpre "Comment details" s fokusom na
besedilu, Tab na Back, Escape vrne fokus na seznam, kontekstni meni se odpre in Escape ga
zapre, iskanje brez zadetkov pokaže "No matching comments." in onemogoči gumbe, Escape na
seznamu zapre okno s fokusom v predvajalniku. Kopiranja v odložišče in odpiranja kanala
avtorja nisem preizkusil živo, ker bi spremenilo Urhovo odložišče in odprlo brskalnik;
pokrivajo ju testi besedila. Pot YouTube Data API je preverjena samo s testi, ker ključ
ni nastavljen.

### E13a: predvajanje v ozadju (29. 9. 2026)

Spremembe pri vklopljeni nastavitvi "Enable background playback":

- Stran predvajalnika ima gumb "Back to main menu Escape", ki pusti predvajanje teči in
  odpre glavni meni, pod njim je vgrajen seznam "Results" zaslona, s katerega se je
  predvajanje začelo (iskanje, Trending, kanal ali seznam predvajanja, mapa), z izbranim
  predvajanim elementom. Tab: Back, Results, predvajalnik, gumbi. Na seznamu delujejo
  Enter, kontekstni meni, bližnjice seznama in bližnjice predvajalnika, ki jih seznam ne
  uporablja sam. Na koncu gumbov je "Close Escape": ustavi predvajanje, oglasi "Player
  closed." in odpre glavni meni.
- V celozaslonskem načinu je edini navigacijski gumb "Back to results Escape": zapusti
  celozaslonski način in postavi fokus na vgrajeni seznam.
- Escape na strani predvajalnika: na predvajalniku in gumbih ustavi predvajanje in vrne
  na prejšnji zaslon, na vgrajenem seznamu in na gumbu Back pusti predvajanje in odpre
  glavni meni (Python `player_escape_closes_playback`).
- Na vseh drugih zaslonih je na koncu razdelek predvajalnika v ozadju: napis "Player:
  naslov", predvajalnik ("Player") in gumbi Previous, Play ali Pause, Next, Playback
  queue, Add to playlist, Audio output devices, Equalizer, Start full screen, Bass boost,
  Repeat, Shuffle, Copy link in Close. Tab ga doseže za kontrolami zaslona, Shift+Tab s
  predvajalnika vrne na seznam ali besedilno polje zaslona. Na predvajalniku in gumbih
  delujejo tipke predvajalnika, preslednica na predvajalniku ustavi ali nadaljuje
  predvajanje, Enter in preslednica na gumbu ga pritisneta, F7, poglavja in podobno
  odprejo stran predvajalnika. Python gumbom sicer nastavi ime "Player: napis", a wx tega
  imena ne pokaže bralnikom zaslona (preverjeno z wx 4.2.5 in MSAA), zato jih tudi Rust
  poimenuje samo z napisom.
- Previous, Next, sorodni video, naslednji element po koncu in element iz vrste
  predvajanja se ob predvajanju v ozadju začnejo brez preklopa zaslona. Na zaslonih brez
  besedilnega polja in izven seznama rezultatov delujejo tudi Ctrl+PageUp, Ctrl+PageDown,
  Ctrl+Shift+PageDown in F11 (Python `handle_active_player_global_shortcut_event`).
- Ko se predvajalnik po predvajanju v ozadju spet pokaže, Back vodi na zaslon, s katerega
  se je predvajanje začelo, in "Resume last session" si ga zapomni (Python
  `player_return_screen`).
- Ctrl+Space brez predvajalnika oglasi "Player not found.".

Popravki, ki veljajo tudi brez predvajanja v ozadju:

- Globalne bližnjice za zaslone (glavni meni, iskanje, priljubljene, zgodovina, mapa,
  datoteka, bookmarks, nastavitve in druge) na strani predvajalnika ustavijo predvajanje,
  kot v Pythonu. Rust je predvajanje prej nevidno nadaljeval.
- "Back to main menu" na strani predvajalnika odpre glavni meni, prej je vodil na
  prejšnji zaslon. Close v kontekstnem meniju in v Action Finderju je Pythonov
  `close_current_player`.
- Enter na gumbu glavnega okna pritisne ta gumb, prej je izbral vrstico seznama ali pa na
  strani predvajalnika ni naredil ničesar (Python `activate_focused_button_from_key`).
- Gumb Play ali Pause nima bližnjice v napisu, kot `current_play_pause_label`.

Predloga za Urha, odobrena 29. 9. 2026 kot O-9 in O-10 (Rust vedenje ostane):

- **P-5.** Ko se predvajanje začne s priljubljenih, zgodovine, podcasta ali seznama
  predvajanja, Python v vgrajenem seznamu pokaže stare rezultate zadnjega iskanja. Rust
  takrat vgrajenega seznama ne pokaže.
- **P-6.** Python ob odprtju nastavitev s strani predvajalnika ustavi predvajanje, kadar
  predvajanje v ozadju ni vklopljeno. Rust nastavitve odpre kot okno nad predvajalnikom in
  predvajanje pusti teči.

Ostaja: vgrajeni seznam ne nalaga dodatnih rezultatov ob koncu seznama, kot jih Python.
Po vrnitvi v glavni meni se rezultati kanala ali seznama predvajanja pozabijo, zato
takrat vgrajenega seznama ni.

Preverjanje: `cargo build`, `cargo test` (509 uspešnih, 12 izključenih), `cargo clippy
--all-targets -D warnings` in `cargo fmt --check`. Novi testi pokrijejo gumbe strani
predvajalnika v treh načinih, razdelek predvajalnika v ozadju, napis Play in Pause,
Close v Action Finderju in vrnitev na začetni zaslon. Samodejni preizkus kopije bete z
ločenimi podatki, glasnostjo 0 in brez knjižnice NVDA je tekel na ločenem nevidnem
namizju, zato ni prevzel ospredja in ga NVDA ni videl. Nadaljevanje seje iz mape pokaže
seznam "Results" z dvema elementoma, Tab vrstni red je Back, Results, Player, Previous,
Pause. Dol in Enter predvajata drugi element, Escape na seznamu odpre glavni meni med
predvajanjem z razdelkom, Tab na glavnem meniju: seznam, Open, Player, Previous, Pause,
Shift+Tab s predvajalnika vrne na seznam, preslednica spremeni gumb v Play in obdrži
fokus, Ctrl+PageUp na seznamu glavnega menija predvaja prejšnji element v ozadju, Enter
na gumbu Previous deluje, gumb celozaslonskega načina odpre predvajalnik z "Back to
results", ta postavi fokus na seznam, Close oglasi "Player closed.", nadaljevanje seje
in Escape na predvajalniku vrneta v mapo. Na iskalnem zaslonu je Tab vrstni red polje,
Type, Search, Back, Player, Previous, Ctrl+PageDown v polju ne preskoči, F7 na
predvajalniku odpre stran s podrobnostmi. Brez predvajanja v ozadju Ctrl+Alt+Y na strani
predvajalnika ustavi predvajanje in odpre iskanje.

### E14: SoundCloud, Shorts in iskalni zaslon (29. 9. 2026)

Spremembe:

- Iskalni zaslon je zgrajen kot Pythonov `show_search`: Back, polje "Search query",
  izbira "Search provider" (YouTube, SoundCloud), izbira "Type", vrstica gumbov Search,
  Play, Download audio, Download video in Add to favorites ter seznam "Result list". Tab
  gre v tem vrstnem redu, Back je pred poljem. Pred iskanjem ima seznam vrstico "No search
  results.", Enter na njej in gumbi brez izbranega rezultata pokažejo "Select an item.",
  kot `play_selected` in `start_download`.
- Rezultati iskanja, vsebina kanala, playlista in izvajalca ter shranjeni novi videi
  naročnine so zdaj prikazani na iskalnem zaslonu, kot v Pythonu. Prej so imeli svoj
  zaslon z gumboma Open in Back. Seznam se tudi pri kanalu imenuje "Result list".
- Back po iskanju vodi v glavni meni, kot `back_from_search`. Prej je vodil nazaj na
  prazen iskalni zaslon. Iz kanala ali playlista Back še vedno vrne na prejšnje
  rezultate. Nov obisk iskanja izprazni polje in izbere YouTube s tipom All.
- Izbira ponudnika zamenja tipe: YouTube All, Video, Playlist, Channel; SoundCloud Track,
  Playlist, User (`on_search_provider_change`). Enter v polju išče tudi, ko so rezultati
  že prikazani. Gumb Search se med iskanjem ne onemogoči več, zato fokus na njem ostane.
- Na polju, izbirah in gumbih iskalnega zaslona delujejo bližnjice s Ctrl ali Alt na
  izbranem rezultatu, navadne tipke ostanejo kontroli (`on_char_hook`).
- SoundCloud skladbe (Track) se iščejo z `scsearch`, vrstice imajo tip "Track",
  izvajalci "Artist". Predvajanje, kopiranje neposrednega URL-ja in prenos gredo skozi
  yt-dlp. Enter ali "Open" v meniju izvajalca odpre njegove skladbe (`/tracks`) z oglasom
  "Loading channel", tudi iz priljubljenih in zgodovine, kjer je Rust prej oglasil "ni na
  voljo". Meni izvajalca je Pythonov: Open, prenos, priljubljene, Open in browser, Copy
  URL, brez naročnine in zavihkov kanala. SoundCloud seti se odprejo kot playlist, njihovo
  število skladb se prebere iz `track_count`.
- Iskanje YouTube All in Video hkrati poišče Shorts in jih, kadar so do konca glavnega
  iskanja že prejeti, vmeša po en Short za vsake štiri rezultate brez podvojitev
  (`youtube_search_results_with_shorts`, `interleave_youtube_results`). Zavihek Videos
  kanala enako vmeša kanalove Shorts, pri čemer počaka na oba seznama
  (`youtube_channel_upload_results`). Preverjanje naročnin še naprej bere samo zavihek
  Videos, kot `fetch_subscription_entries`.

Odločitev za Urha:

- **P-7** (odobreno 29. 9. 2026 kot O-11, narejeno v E15). Python SoundCloud playliste in uporabnike išče prek spletnega API-ja
  SoundClouda z javnim ID-jem spletnega odjemalca, ki ga yt-dlp pobere s strani
  soundcloud.com. Samostojni yt-dlp.exe te poti nima, zato bi moral Rust ta ID pobrati sam.
  Samodejna varovalka mojega okolja mi je pregled tega ID-ja ustavila, zato te poti nisem
  naredil. Do odločitve tipa Playlist in User pri SoundCloudu pokažeta angleško napako
  "SoundCloud playlist and user search is not available yet".

Preverjanje: `cargo build`, `cargo test` (vsi uspešni), `cargo clippy --all-targets -D
warnings` in `cargo fmt --check`. Novi testi pokrijejo vmešavanje Shorts po Pythonovem
vrstnem redu, SoundCloud vnose, cilje iskanja in skladb izvajalca, tipe po ponudniku,
zaslone z rezultati, oznake vrstic in meni izvajalca. Živi test z yt-dlp je potrdil
SoundCloud iskanje, razrešitev skladbe, skladbe izvajalca in Shorts v zavihku Videos.
Samodejni preizkus kopije bete na ločenem nevidnem namizju je potrdil Tab vrstni red,
sporočilo na prazni vrstici, menjavo tipov, iskanje in predvajanje SoundCloud skladbe z
gumbom Play, Back v glavni meni, prazen nov obisk iskanja, YouTube iskanje ter odprtje
SoundCloud izvajalca iz priljubljenih z vrnitvijo v priljubljene.

### E15: diagnostično poročilo in SoundCloud playlisti ter uporabniki (29. 9. 2026)

Spremembe:

- O-11 (P-7): `apricot-platform/src/soundcloud_search.rs` išče SoundCloud playliste
  (`search/playlists_without_albums`) in uporabnike (`search/users`) prek
  `api-v2.soundcloud.com` s Pythonovimi parametri in straničenjem `next_href`. ID
  odjemalca prebere iz skript domače strani kot `yt-dlp`, sprejme le skripte s
  soundcloud.com in sndcdn.com ter naslednje strani le z `api-v2.soundcloud.com`.
  Napake ID-ja ne izpišejo. Vrstice playlistov dobijo lastnika iz vgnezdenega `user`
  (ime in `channel_url`), kot `normalize_entry`. Kanal vnosa se zdaj bere v Pythonovem
  vrstnem redu `uploader`, nato `channel`.
- Ctrl+Alt+Shift+D, postavka glavnega menija in Action Finder sestavijo Pythonovo
  diagnostično poročilo (`apricot-app/src/diagnostic_report.rs`) z enakimi razdelki,
  oznakami in oblikovanjem vrednosti (yes/no, none, decimalke brez ničel, omejitev
  1000 znakov), ga kopirajo in oglasijo "Diagnostic report copied.". Zakrivanje je
  Pythonovo: naslovi brez poizvedbe, fragmenta in poverilnic, polja Cookie,
  Authorization, Proxy-Authorization, X-Api-Key in youtube_data_api_key ter mape
  APPDATA, LOCALAPPDATA, USERPROFILE in TEMP brez razlikovanja velikih črk, najdaljša
  najprej. Naslov toka se povzame brez poizvedbe. Repi `mpv.log` in `updater.log`
  (zadnjih 50 vrstic iz največ 256 KiB).
- Različico yt-dlp in repe dnevnikov prebere delovna nit, zato okno med kopiranjem ne
  zamrzne; poročilo se kopira, ko je sestavljeno (običajno v pol sekunde).
- libmpv zdaj piše `mpv.log` v podatkovno mapo, kot Python piše izhod mpv: sporočila
  ravni info in višje v obliki terminala, datoteka se ob vsakem novem posnetku prepiše.
- Nujne razlike v vsebini poročila: vrstice "Python" ni, "mpv path" kaže `libmpv-2.dll`,
  "Process PID" je PID aplikacije (mpv teče v procesu), "Stream header names" je vedno
  "none", "Return index" je "none", "Cookies source refresh error" je prazen do E16,
  vrstice "mpv ..." pa se berejo iz stanja seje namesto sprotnega branja lastnosti.
  "App label" je "2 Beta" z različico, kot v naslovu okna.

Preverjanje: `cargo build`, `cargo test` (530 uspešnih), `cargo clippy --all-targets -D
warnings` in `cargo fmt`. Novi testi pokrijejo Pythonov varnostni test zakrivanja,
oblikovanje vrednosti, povzetek naslova toka, repe dnevnikov, sestavo razdelkov z odprtim
predvajalnikom, vrstice `mpv.log`, opis platforme, poizvedbe in gostitelje SoundCloud API
ter lastnika playlistov. Živa testa s pravim SoundCloudom vrneta po 25 oziroma 5
playlistov in uporabnikov. Kopija bete na ločenem nevidnem namizju je potrdila iskanje
playlistov in uporabnikov z vrsticami "Playlist" in "Artist", predvajanje skladbe, `mpv.log`,
kopiranje poročila s Ctrl+Alt+Shift+D in iz glavnega menija brez premika fokusa.
Uporabnikovo odložišče je driver po testu obnovil.

Še odprto: postavka "Type" trenutnega elementa je prazna, kadar element nima shranjenega
prikaznega tipa (na primer SoundCloud skladba iz iskanja); Python tam napiše "Track".
Odstopanje iz E13 (odrezovanje števila ogledov v F7) ostaja, ker E15 tega dela ne spreminja.

### E16: piškotki (29. 9. 2026)

Spremembe:

- Jedro piškotkov je v `apricot-app/src/cookies.rs`, prenos Pythonovega `CookiesUI`:
  branje Netscape `cookies.txt` kot `MozillaCookieJar` (glava, `#HttpOnly_`, vrstice brez
  imena, isti zapis in vrstni red pri shranjevanju), popravljanje s presledki ločenih
  vrstic, JSON izvozi razširitev (gnezdeni seznami, domene iz ključev in naslovov,
  `expirationDate` v milisekundah), glava `Cookie:`, varnostni filter kontrolnih znakov,
  filter domen YouTube in Google, točkovanje in zaznava prijavnih piškotkov. V
  predpomnilnik `cookies.txt` v podatkovni mapi bete se kot v Pythonu zapišejo samo
  piškotki YouTube in Google, z atomsko zamenjavo datoteke.
- `effective_cookies_file` je Pythonov: izvorna datoteka se ob spremembi SHA-256 podpisa
  ali praznem predpomnilniku znova uvozi, ročno vpisana pot se uvozi v predpomnilnik,
  stara pot v mapi Dokumenti se enkrat preseli. Uvožene Pythonove nastavitve, ki kažejo
  na Pythonov `cookies.txt`, se prepišejo v predpomnilnik bete, Pythonova datoteka pa
  ostane nespremenjena. Napaka osvežitve je v diagnostičnem poročilu v vrstici
  "Cookies source refresh error".
- Nastavitve, razdelek Cookies and network: polje prikaže izvorno pot, "Choose cookies.txt
  file" uvozi datoteko s Pythonovimi sporočili (JSON, glava, Netscape, prijava najdena
  ali opozorilo brez prijave), izbira brskalnika se vrne na none in profil na Auto.
  Seznam profilov je Pythonov (`Auto - try all profiles`, najdeni profili brskalnika,
  shranjena vrednost). "Open YouTube in selected profile" odpre Chromium brskalnik s
  profilom ali privzeti brskalnik. "Obtain YouTube API key" odpre stran Google Cloud
  Credentials. "Export browser cookies to cookies.txt" vpraša za zaprtje odprtega
  brskalnika, izvozi na delovni niti in na koncu posodobi polje, izbiro brskalnika in
  oglasi "Browser cookies exported to ... from ...". Ponastavitev razdelka in obnovitev
  privzetih nastavitev izbrišeta predpomnilnik piškotkov kot v Pythonu.
- Izvoz iz brskalnika (`apricot-platform/src/browser_cookies.rs`) sledi
  `export_browser_cookies_blocking`: kandidati profilov, ponovni poskus po zaklenjeni bazi
  z zaprtjem brskalnika, izbira najboljšega profila po točkah in DevTools nadomestek za
  Chromium brskalnike razen Chroma (zagon z `--remote-debugging-port`, preverjen lokalni
  WebSocket, `Network.getCookies`). Namesto knjižnice `yt_dlp.cookies` Rust kliče
  `yt-dlp.exe --cookies-from-browser ... --cookies <začasna datoteka>` brez naslova;
  yt-dlp piškotke ob izhodu shrani, izhodna koda 2 zaradi manjkajočega naslova pa se ne
  šteje za napako. Diagnostika neuspeha ima Pythonovo obliko.
- yt-dlp se zdaj kliče kot v Pythonu: iskanje, zbirke, metapodatki, prepisi in komentarji
  najprej brez piškotkov, ob napaki prijave s piškotki, nato po samodejni osvežitvi iz
  brskalnika (`repair_cookies_for_error`, oglasi "YouTube needs sign-in cookies. Refreshing
  cookies from ...", po neuspehu pet minut premora). Prej je Rust piškotke dodal vsakemu
  klicu. Razrešitev predvajanja poskusi s piškotki le ob napaki prijave, starosti ali
  predvajalnika in pri YouTube le, če datoteka vsebuje prijavo. Prenosi po neuspelem
  poskusu s piškotki prav tako osvežijo piškotke iz brskalnika. `cookie_user_agent` se
  pošlje samo skupaj s piškotki.
- Napaka predvajanja zdaj pokaže Pythonovo besedilo "Player did not start: ..." z
  namigi `friendly_error`. Pri napaki prijave, vklopljeni podpori za starostno omejene
  videe in izbranem brskalniku Rust kot Python vpraša "Refresh YouTube cookies", po
  potrditvi osveži piškotke in predvajanje ponovi, če se medtem ni začelo drugo.
- Popravljeno odprto odstopanje iz E15: vrstica "Type" v diagnostičnem poročilu za
  YouTube in SoundCloud element brez shranjenega tipa izpelje Pythonov prikazni tip
  (Track, Artist, Playlist, Channel, Live stream, Video).

Nujne razlike: izvoz teče prek `yt-dlp.exe` namesto knjižnice, zato so besedila napak
zadnja vrstica `ERROR:` iz yt-dlp z opozorili; opisi sistemskih napak (na primer manjkajoča
datoteka) so Windows besedila namesto Pythonovih `[Errno ...]`. Pythonovih dodatnih
poskusov razrešitve (nadomestni format, odjemalec web_safari, JS rešitelj) E16 ne prenaša.

Preverjanje: `cargo build`, `cargo test` (561 uspešnih), `cargo clippy --all-targets -D
warnings` in `cargo fmt`. Novi testi pokrijejo branje in zapis Netscape datotek (izhod je
enak Pythonovemu `MozillaCookieJar`), zavrnitve, JSON in glavo, točkovanje, uvoz v
predpomnilnik, osvežitev izvora, prepis Pythonove poti, selitev iz Dokumentov, ročno
vpisano pot, profile, algoritem izvoza z zaklenjeno bazo in DevTools, WebSocket odjemalec,
argumente yt-dlp s piškotki in vrstico "Type". Živi test z `yt-dlp.exe` je izvozil
ponarejen Firefox profil. Kopija bete na ločenem nevidnem namizju z ločenimi podatki je
potrdila razdelek s profili, uvoz JSON datoteke (fokus ostane na gumbu), opozorilo brez
prijave, izvoz iz ponarejenega Chromium profila z oglasom in posodobljenimi kontrolami,
Pythonovo sporočilo neuspeha za Opero brez profila, ponastavitev razdelka z izbrisom
predpomnilnika ter YouTube iskanje in predvajanje brez piškotkov. Resnični brskalniki
niso bili zaprti ali brani.

Še odprto: odstopanje iz E13 (odrezovanje števila ogledov v F7) ostaja, ker E16 tega dela
ne spreminja.

### E17: pretvornik datotek in pretvornik map (29. 9. 2026)

Spremembe:

- "File converter" in "Folder converter" iz glavnega menija in iskalnika dejanj odpreta
  dialog Pythonovega `show_converter_dialog` (`apricot-ui-windows/src/converter_win32.rs`)
  namesto oglasa, da funkcija v beti ni na voljo (PODDL-M-01). Kontrole so v Pythonovem
  vrstnem redu, ki je tudi vrstni red Tab: pot, Browse file ali Browse folder, izbira
  formata, pri pretvorbi zvoka v video Add image in Dark background, pri sliki še pot do
  slike in Choose image, Create a new file in Replace original file (za mapo Pythonova
  besedila za mapo), Convert in "Back to main menu". Začetni fokus je na poti, Escape in
  Back zapreta dialog, fokus se vrne na glavni meni. Enter pritisne gumb s fokusom.
- Logika je v `apricot-app/src/converter.rs`: seznama formatov (pri video vhodu najprej
  video formati), oznaka "ALAC (M4A)", ffmpeg argumenti enaki Pythonovim
  `converter_ffmpeg_args`, `converter_audio_codec_args` in `converter_video_codec_args`
  (črno ozadje 1280x720 ali slika z `-loop 1`), privzeto ime izhoda, "ime (2).končnica" za
  nov izhod, "mapa converted" oziroma "mapa converted (2)" za nove mape, rekurzivni pregled
  mape v Pythonovem vrstnem redu brez delovnih datotek `.apricot-converting`, filtri
  datotečnih dialogov in delavca za datoteko in mapo. Zamenjava izvirnika gre prek skrite
  sosednje datoteke, izvirnik se izbriše šele po uspehu.
- Preverjanja in sporočila so Pythonova: "Select an item.", "Choose image for video
  background", "This input format is not supported.", preklic dialoga za shranjevanje ali
  mapo oglasi "Conversion cancelled." in pusti pretvornik odprt. Ob začetku se oglasi
  "Conversion started.", med pretvorbo mape vrstica stanja kaže "Conversion started. 1/2:
  ime". Konec pokaže sporočilno okno ali, če je `popup_when_conversion_complete` izklopljen,
  oglasi "Conversion complete: ..." oziroma "Folder conversion complete: ...". Napaka
  pokaže "Conversion failed: ..." z zadnjimi 900 znaki izpisa ffmpeg in namigi
  `friendly_error`.
- Pretvorba mape odpre nemodalno okno napredka "Converting folder" s Pythonovim besedilom
  (datoteka, Converted: x of y, Remaining: z), vrstico napredka ter pretečenim,
  ocenjenim in preostalim časom. Okno je pravi sistemski dialog brez gumbov, zato bralnik
  zaslona ob prikazu prebere naslov in besedilo, kot pri wx `ProgressDialog`. Ob zaprtju se
  fokus vrne tja, kjer je bil.

Nujne razlike: wx na Windows za `ProgressDialog` uporablja sistemski Task Dialog, ki
zahteva Common Controls 6. Rust beta tega manifesta nima, zato je okno napredka navaden
sistemski dialog z enako vsebino. Besedila o času so kot v wx angleška.

Predlogi za Urha (Python obnašanje, ki je videti kot majhna napaka, Rust ga zaenkrat
posnema):

- **P-8** (odobreno 29. 9. 2026 kot O-12). Pri neuspeli pretvorbi ene datoteke Python pusti delno izhodno datoteko, pri
  zamenjavi izvirnika pa skrito datoteko ".ime.apricot-converting-...". Rust bi ju lahko
  izbrisal, kot to Python že naredi pri pretvorbi mape z zamenjavo.
- **P-9** (odobreno 29. 9. 2026 kot O-13). Ko uporabnik v dialogu za shranjevanje potrdi prepis obstoječe datoteke, Python
  vseeno zapiše "ime (2).končnica". Rust bi lahko prepisal izbrano datoteko.

Preverjanje: `cargo build`, `cargo test` (571 uspešnih, 15 izključenih), `cargo clippy
--workspace --all-targets -D warnings` in `cargo fmt`. Novi testi pokrijejo vrste vhoda,
vrstni red formatov, poti izhodov, edinstvena imena, ffmpeg argumente, filtre, pregled
mape, oba delavca z zamenjavo in napakami, besedila napredka ter predlogo dialoga. Kopija
bete na ločenem nevidnem namizju z ločenimi podatki in pravim ffmpeg je potrdila začetni
fokus in vrstni red Tab (tudi z možnostmi slike), opozorilo pri prazni poti s fokusom na
Convert, prikaz možnosti za video in preklop Add image in Dark background, pretvorbo WAV v
MP4 s sliko (privzeto ime "song.mp4" v dialogu), preklic s "Conversion cancelled.",
zamenjavo izvirnika brez ostankov, Escape z vrnitvijo fokusa na glavni meni, pretvorbo
mape v "music converted" s podmapo in oknom napredka ter pretvorbo mape z zamenjavo
izvirnikov. Po vsakem koncu je fokus na glavnem meniju.

Še odprto: odstopanje iz E13 (odrezovanje števila ogledov v F7) ostaja, ker E17 tega dela
ne spreminja.

### E18: posodobitve yt-dlp in aplikacije (29. 9. 2026)

Najprej sta narejena odobrena O-12 in O-13 iz E17 (pretvornik): neuspela pretvorba ene
datoteke izbriše delno izhodno ali skrito začasno datoteko, potrjen prepis v dialogu za
shranjevanje pa res prepiše izbrano datoteko prek skrite delovne datoteke.

Spremembe E18:

- `apricot-updater` ima Pythonovo logiko: `parse_version`, `is_newer_version` in
  `is_component_version_newer`, branje GitHub izdaj s kanaloma stable in beta
  (`fetch_latest_release`, `fetch_public_releases`, zbirni seznam novosti z mejo 12000 znakov
  in besedilo izdaje z mejo 6000 znakov), izbiro paketa glede na nameščeno ali prenosno
  različico, preverjanje zaupanja naslovov, velikosti in SHA-256, preverjanje zip paketa
  (število, velikost in razmerje stiskanja članov, nevarne poti, šifrirani in posebni
  vnosi), skripte za posodobitev s prenosnim zipom in z namestitvenim programom
  (čakanje na izhod, preverjanje zgoščene vrednosti, varnostna kopija, rollback, ponovni
  zagon z `--updated-relaunch`), zagon skripte v skritem PowerShellu in `updater.log` z
  obrezovanjem pri 2 MiB.
- Omrežje je v `apricot-platform/src/app_update.rs` (samo HTTPS preusmeritve, končni
  naslov mora biti pod github.com oziroma githubusercontent.com, meje velikosti).
- yt-dlp: ob zagonu po 3,5 sekunde in z gumbom "Check yt-dlp updates now" se preveri
  najnovejša uradna izdaja. Novejši `yt-dlp.exe` gre v `components` v podatkovni mapi
  (Pythonov `COMPONENTS_DIR`) in ima prednost pred priloženim. Oglasi so Pythonovi:
  "Checking updates for YouTube support.", "Updating components.", "Components updated.",
  "YouTube support is up to date." (samo ročno) in "Could not check YouTube support
  updates: ...".
- Aplikacija: ob zagonu po 5,5 sekunde (z vprašanjem), vsakih `app_update_interval_hours`
  (brez vprašanja: postavka "Update available: X" na vrhu glavnega menija, stanje in
  obvestilo, ko aplikacija nima fokusa) in z gumbom "Check for updates" (shrani nastavitve
  in vpraša tudi za preskočeno verzijo). Dialog "Update available" ima Pythonove kontrole:
  "Version X", "What's new?", polje samo za branje z imenom "What's new?" in začetnim
  fokusom, "Would you like to update now?", "Update now" (privzeti gumb) in "Skip this
  version". Escape in zapiranje pomenita preskok, ki se shrani v `skipped_update_version`.
  Prenos pokaže okno napredka "Updating ApricotPlayer" s pretečenim in ocenjenim časom,
  nato se zažene skripta in aplikacija se zapre.
- Ponovni zagon z `--updated-relaunch` 45 sekund ne pokaže sporočila, da je aplikacija že
  odprta (Python `suppress_already_open_for_update`).

Nujne razlike:

- Rust uporablja samostojni `yt-dlp.exe`, zato se posodobi iz uradnih GitHub izdaj yt-dlp
  (SHA-256 iz podatkov izdaje ali iz `SHA2-256SUMS`), ne iz PyPI paketa.
- Rust paket ima drugačno obliko kot PyInstaller: prenosni zip ima korensko mapo z
  izvršljivo datoteko ter mapami `components`, `mpv`, `ffmpeg`, `assets` in `nvda`. Imena
  paketov za beto (`ApricotPlayer2Beta.zip`, `ApricotPlayer2BetaSetup.exe`) so začasna in se
  določijo ob pripravi izdaje 2.0 (E20).
- Lokalna beta (D-011) preverja GitHub kot Python, a namesto namestitve pokaže Pythonovo
  sporočilo "Automatic install works only in the .exe build. New release available: X".
  Samo z okoljsko spremenljivko `APRICOT_UPDATE_TEST_FEED` (mapa z `latest.json`,
  `releases.json`, `ytdlp-latest.json` in paketi) lokalna beta namesti posodobitev, za
  preizkuse.
- Časovnik posodobitev se ponastavi, ko se zaprejo nastavitve, ne ob Save, kot že pri
  časovniku naročnin.
- Pythonova tretja skripta (zamenjava samo .exe) ni prenesena, ker je Python nikoli ne
  uporabi: dovoljena imena paketov so samo zip in namestitveni program.

Opozorilo za izdajo 2.0: Pythonov `parse_version` oznako "dev" razvrsti kot končno
izdajo, zato "2.0.0" ni novejša od "2.0.0-dev.1". Javne predizdaje naj uporabljajo
"alpha", "beta" ali "rc".

Preverjanje: `cargo build`, `cargo test` (596 uspešnih, 16 izključenih), `cargo clippy
--workspace --all-targets -D warnings` in `cargo fmt`. Novi testi pokrijejo primerjavo
verzij, izbiro izdaj po kanalih, zbirni seznam novosti, izbiro in imena paketov,
preverjanje naslovov, velikosti, SHA-256 in zipa, obe skripti, dnevnik, preverjanje in
prenos z lažnim GitHubom ter oba primera O-12 in O-13. Izključeni test v živo je enkrat
prenesel pravi `yt-dlp.exe` z GitHuba v začasno mapo. Kopija bete na ločenem nevidnem
namizju z ločenimi podatki in lokalnim testnim virom je potrdila napako pri napačni
zgoščeni vrednosti, začetni dialog (imena, vloge, fokus v polju, Tab, Enter v polju ne
stori ničesar, Escape preskoči in shrani), ročni preverjanji yt-dlp iz nastavitev (posodobitev
in "up to date"), ročni dialog nad nastavitvami s fokusom nazaj na gumbu, postavko "Update
available: 2.0.0-dev.3" po časovniku ter celotno posodobitev z zamenjavo datotek, brez
ostankov in s ponovnim zagonom z `--updated-relaunch`. Na pravem GitHubu je zagonsko
preverjanje v kopiji javilo "The app is up to date." (izdaje 1.x niso novejše).

### E19: AudioVault (30. 9. 2026)

Spremembe:

- Ctrl+Alt+A in postavka AudioVault v glavnem meniju odpreta Pythonov AudioVault meni
  (gumba "Back to main menu" in "Open", seznam z imenom "AudioVault": "Search", "View
  recently added TV shows", "View recently added movies"). Brez prijave se najprej
  pokaže okno "Log in to AudioVault" (Email, Password, Register, OK, Cancel; fokus na
  Email, Enter v obeh poljih pomeni OK, Escape prekliče), s shranjenim geslom pa se
  prijava zgodi sama. Napačno shranjeno geslo pokaže sporočilo, pozabi geslo in odpre
  okno; napačno vneseno geslo pokaže samo sporočilo, kot v Pythonu.
- Geslo se shrani z DPAPI in isto entropijo kot v Pythonu, zato Rust prebere geslo, ki
  ga je shranila Python verzija (preverjeno z izključenim testom na pravem Pythonovem
  zapisu). Sprememba e-pošte v nastavitvah pozabi geslo in odjavi, kot Python.
- Iskalni zaslon (Back, Search query, Type z Movies in TV shows, Search, Play, Download
  audio, seznam), zaslon nedavnih naslovov (Back, Play, Download audio, naslov, seznam z
  imenom naslova), Pythonov vrstni red Tab, vrstice "Naslov | Movie", "Naslov | TV show"
  in "Epizoda | Episode", kontekstni meni Open ter "Download audio" s kratico ali "Download
  TV show", Ctrl+Shift+D oglasi "Video is unavailable from AudioVault. Download audio
  instead.".
- TV serija: seznam epizod se prebere iz oddaljenega zipa z branjem po delih (Range),
  epizode so v naravnem vrstnem redu, predvajana epizoda se prenese v predpomnilnik
  (`cache\audiovault\<id>\_episodes`), star predpomnilnik se obreže na `cache_size_mb`.
  Kadar strežnik ne podpira Range, se prenese in varno razširi cel paket (80 odstotkov
  prenos, 20 odstotkov razširjanje). Naslednja in prejšnja epizoda v predvajalniku
  pripravita še nepreneseno epizodo in jo predvajata.
- Film se predvaja iz razrešenega naslova s piškotkom seje, User-Agent in Referer (mpv
  dobi glave prek `http_headers`); glave se nikoli ne shranijo v zgodovino, priljubljene
  ali zadnjo sejo.
- Prenosi: film v `Downloads\AudioVault`, epizoda v `AudioVault\<serija>`, serija v
  `AudioVault\<serija>`, z vprašanjem za mesto, kadar je vklopljeno. Oglas "AudioVault
  download complete: X. Saved to Y".
- Iztek seje (preusmeritev na prijavo) sproži ponovno prijavo in ponovi dejanje, kot
  Python. Nastavitve: "Log in to AudioVault" (nad oknom nastavitev), "Log out of
  AudioVault" in "Register".
- Nazaj s predvajalnika obnovi AudioVault zaslon po Pythonovem
  `restore_audiovault_player_results` (epizode dobijo zaslon z naslovom serije), Escape
  in Back sledita `back_from_audiovault`.

Nujne razlike:

- Besedila omrežnih napak (na primer brez povezave) prihajajo iz Rust HTTP knjižnice;
  "HTTP Error 404: Not Found" in Pythonova angleška besedila o varnosti zipa ostanejo enaka.
- Bralnik zipa podpira shranjene in stisnjene (deflate) člane, kar AudioVault uporablja;
  Pythonov `zipfile` bi znal še bzip2 in lzma.
- Rezultati, ki pridejo po tem, ko je uporabnik zaslon že zapustil, se zavržejo (Python bi
  pisal v uničen ali drug seznam).
- Film iz zgodovine ali priljubljenih se razreši prek AudioVault seje (s ponovno prijavo);
  Python bi ga poskusil predvajati prek yt-dlp.
- Sprotni odstotki prenosa se samo izpišejo v vrstici stanja, ne oglasijo, da NVDA ne
  bere vsakega odstotka.
- Za preizkuse lokalna beta (ne izdaja) sprejme `APRICOT_AUDIOVAULT_TEST_BASE` z
  naslovom na 127.0.0.1.

Predlog, že narejen:

- **P-10** (odobreno 30. 9. 2026 kot O-14). Python ob novem AudioVault zaslonu obdrži prejšnje rezultate, zato Play ali
  Download audio na vrstici "No search results." predvaja ali prenese prvi element
  prejšnjega seznama. Rust ob novem zaslonu rezultate počisti, tako da gumba ne storita
  ničesar, dokler ni novih rezultatov. Hkrati fokus po kontekstnem meniju ostane na
  seznamu.

Preverjanje: `cargo build`, `cargo test` (613 uspešnih, 17 izključenih), `cargo clippy
--workspace --all-targets -D warnings` in `cargo fmt`. Novi testi pokrijejo razčlenjevanje
strani kot Pythonov HTMLParser, polja rezultatov in epizod, poti predpomnilnika in
prenosov, `safe_folder_name`, naravni vrstni red, varnost zipa, bralnik zipa (tudi napačen
CRC), branje po delih, razširjanje, obrezovanje predpomnilnika, DPAPI, glave za mpv in
kontekstni meni. Kopija bete na ločenem nevidnem namizju je proti lokalnemu lažnemu
AudioVault strežniku preverila vse zgoraj: prijavo in napake prijave, menije, fokus, imena
in vrstni red Tab, nedavne naslove, iskanje, epizode, predvajanje epizod in filma (strežnik
je videl piškotek iz mpv), naslednjo in prejšnjo epizodo, vrnitev iz predvajalnika, prenose,
iztek seje in odjavo. Prave AudioVault prijave nisem preizkusil, ker nimam računa.

### E20: zaključna parity vrata (30. 9. 2026)

Rezultat: vsa strojna preverjanja so uspešna, vrata pa še niso zaprta, ker ostajajo
preizkusi, ki jih lahko opravi samo Urh (NVDA celote, pravi računi) ali ki zahtevajo
dolge teke in pripravo izdaje.

Preverjeno s skripto proti Pythonu 1.0.21 (imena, vrstni red in vrednosti, ne samo
števila):

- 116 polj nastavitev, 91 dejanj s privzetimi kraticami in vrstnim redom v seznamu
  kratic, 19 postavk glavnega menija z enakimi ključi besedil, 27 jezikov z imeni,
  10 pasov EQ in 18 tovarniških nastavitev z enakimi vrednostmi ter 3 prosta mesta.
- Pythonove datoteke jezikov so nespremenjene, 5 besedil samo za Rust obstaja v vseh
  27 jezikih in ne prekriva Pythonovih ključev, vsi ključi, ki jih Rust uporablja
  (170), obstajajo, nadomestna mesta `{...}` so v vseh jezikih enaka.
- Nobeno dejanje in nobena postavka glavnega menija ne pade več na sporočilo "ni na
  voljo v tej beti".

Najdeno in popravljeno:

- Zaslon elementov uporabniškega playlista je imel tri gumbe, ki jih Python nima
  ("Play playlist", "Shuffle playlist", "Add to playback queue"). Zadnji je ob uporabi
  izgovoril surova ključa `playback_queue_added_count` in `playback_queue_exists`, ki ne
  obstajata. Gumbi so odstranjeni, zaslon ima Pythonove gumbe Back, Play, Download
  playlist in Remove from playlist ter enak vrstni red Tab.
- Gumb "Check subscriptions now" v nastavitvah je še kazal "ni na voljo". Zdaj kot v
  Pythonu požene ročno preverjanje naročnin; rezultat se oglasi, medtem ko nastavitve
  ostanejo odprte in fokus ostane v njih (preverjeno v kopiji s tvojimi naročninami:
  "1 new videos from SomeOrdinaryGamers.").
- Zagon z `--qualification-smoke` se je sesul, ker je še pričakoval 117 nastavitev
  (pred odstranitvijo `youtube_backend`). Zdaj 116.

Podatki: kopija tvojih pravih Python podatkov je šla skozi iste krmilnike kot prvi
zagon bete (nov test `python_data_import`, zaženeš ga z `APRICOT_PYTHON_APP_DATA`):
zgodovina 131, obvestila 200, naročnine 5, RSS viri 4 s 1205 epizodami, playlist 1 s 4
elementi, zadnja seja prebrana, nobena nastavitev ni spremenjena. Obstoječi preizkus
povratnega zapisa (`qualify_python_data_compat.ps1`) je uspešen.

Zmogljivost (izdajna beta na ločenem namizju, s tvojimi podatki): glavni meni po 227 ms
ob prvem zagonu z uvozom, nato okoli 145 ms, 41 MB delovnega pomnilnika in 18 MB
zasebnega. Primerjave s Pythonom nisem izmeril, ker bi Python med zagonom govoril prek
tvojega NVDA.

Manifest: potrjene točke so odkljukane z dokazom (števila, AudioVault, uvoz, zagon).
Preostale neodkljukane točke potrebujejo spodnji ročni preizkus, prave račune, dolge
teke (8 ur, 14.000 datotek) ali pripravo izdaje 2.0 (namestitev, odstranitev,
podpisani paketi).

Ročni NVDA preizkus celote (primerjaj vsak korak s Python verzijo):

1. Glavni meni: puščice, črke, Enter na vsaki postavki, Escape nazaj; Ctrl+Alt+M,
   Ctrl+Alt+Y, Ctrl+Alt+A, Ctrl+Alt+O, Ctrl+Alt+I, Ctrl+Alt+S od koder koli.
2. Iskanje: vpis, Enter, Tab po vseh kontrolah, Enter na rezultatu, Aplikacije in
   Shift+F10 na rezultatu, kanalu in playlistu, Ctrl+Shift+A in Ctrl+Shift+D.
3. Predvajalnik: presledek, puščice, Ctrl+puščice, T, V, S, D, F4, O, B, Ctrl+PageDown,
   Escape; enako s predvajanjem v ozadju.
4. Priljubljene, zgodovina, playlisti (nov zaslon brez treh gumbov), vrsta za
   predvajanje, zaznamki, center obvestil.
5. Naročnine in podcasti: odpiranje, nove epizode, prenos, Aplikacije meni.
6. Prenosi: posamezni in zbirke, okno napredka, preklic.
7. Nastavitve: vsi razdelki, Tab, ponastavitev, shranjevanje, "Check subscriptions
   now", posodobitve, piškotki, AudioVault prijava in odjava.
8. Pladenj: zapiranje v pladenj, obnovitev, drugi zagon, odpiranje datoteke z
   dvoklikom.

Preverjanje: `cargo build`, `cargo test` (613 uspešnih, 18 izključenih), `cargo clippy --workspace
--all-targets -D warnings`, `cargo fmt`, `--qualification-smoke` in živi preizkusi na
ločenem nevidnem namizju z ločenimi podatki.

### Priprava izdaje 2.0.0-beta.1 (30. 9. 2026)

Vse je lokalno na veji rust-2.0: nič ni potisnjeno, nič ni na main, ni GitHub izdaje.

- Verzija je `2.0.0-beta.1` (Pythonov `parse_version` "dev" šteje kot končno izdajo,
  "beta" pa pravilno kot predizdajo pred 2.0.0).
- Izdajna gradnja ima funkcijo `apricot-updater/release-beta`: kanal posodobitev je
  Beta, zato distribuirana beta namešča poznejše 2.0 predizdaje z GitHuba. Lokalna beta
  (`build_local_beta.ps1` brez `-Channel beta`) ostane LocalOnly (D-011). Testni
  okoljski spremenljivki (`APRICOT_UPDATE_TEST_FEED`, `APRICOT_AUDIOVAULT_TEST_BASE`) v
  izdajni gradnji ne delujeta.
- `rust/scripts/build_release.ps1` iz čistega drevesa zgradi v
  `rust/local-dist/release/<verzija>`: `ApricotPlayer2Beta.zip` (prenosna, ena
  korenska mapa), `ApricotPlayer2BetaSetup.exe` in `SHA256SUMS.txt`. Poganja teste,
  clippy in `--qualification-smoke`.
- `installer/ApricotPlayer2Beta.iss`: namestitev na uporabnika brez skrbniških pravic
  v `%LOCALAPPDATA%\Programs\ApricotPlayer2Beta`, lasten AppId (Python namestitev in
  njen AppId ostaneta nedotaknjena), opravili "desktopicon" in "mediaassoc" z istimi
  imeni, kot ju uporablja skripta za posodobitev. Registracija predvajalnika zapiše
  iste vrednosti v HKCU kot "Set default player" v nastavitvah (seznam da aplikacija z
  `--qualification-media-associations`); odstranitev jih pobriše.
- Osnutek opomb: `release-notes/v2.0.0-beta.1.md`.

Preverjeno: oba paketa preideta preverjanje, ki ga updater izvede ob prenosu posodobitve
(nov izključen test `release_files` z `APRICOT_RELEASE_DIR`). Tiha namestitev v ločeno
mapo je namestila vse datoteke in `unins000.exe`, vpis za odstranitev (verzija
2.0.0-beta.1), bližnjico v meniju Start in registracijo predvajalnika; nameščen program
prestane `--qualification-smoke`, `build-info.json` pravi kanal beta, čista gradnja. Tiha
odstranitev je pobrisala mapo, vpis, bližnjico in vse vrednosti v registru, podatki v
`%APPDATA%\ApricotPlayer2Beta` so ostali. Tvoja Python namestitev in obstoječa beta
nista bili spremenjeni.

Pred objavo (tvoja odločitev): NVDA preizkus celote, podpis paketov, GitHub predizdaja z
obema paketoma in odločitev, ali 2.0 pozneje nadomesti Python 1.x (ista mapa, podatki in
AppId) ali ostane ločena. Python 1.x na kanalu beta bo predizdajo 2.0 videl; pred objavo je
treba preveriti, kaj naredi, ko v njej ne najde svojega paketa `ApricotPlayerSetup.exe`.

### Rust 2.0 nadomesti Python (30. 9. 2026)

Odločitev (Urh, 30. 9. 2026): ApricotPlayer 2.0 nadomesti Python verzijo in ne teče ob
njej. Ob objavi bo 2.0 izšla s Pythonovimi imeni paketov (`ApricotPlayerSetup.exe`,
`ApricotPlayer.zip`), da jo nameščene 1.x najdejo in namestijo čez sebe.

Preverjeno v Pythonovem updaterju: na kanalu stable Python predizdaj ne vidi. Na kanalu
beta predizdajo 2.0 vidi kot novejšo; z imeni `ApricotPlayer2Beta*` v njej ne najde paketa,
javlja "no Windows asset found in release" in ne dobi več nobene 1.x posodobitve. S
Pythonovimi imeni in istim AppId pa tiho s skrbniškimi pravicami zažene namestitveni
program v isto mapo in nato `ApricotPlayer.exe`. Prenosna 1.x se na 2.0 ne more
posodobiti sama, ker zahteva mapo `_internal`.

Spremembe:

- Funkcija `stable-identity` (apricot-player, apricot-platform, apricot-updater): Pythonove
  mape (`%APPDATA%\ApricotPlayer`, predpomnilnik, prenosi, stari
  `UrhasaurusYouTubePlayer` kot vir uvoza), `ApricotPlayer.exe`, ime enojnega primerka,
  vnos za zagon z Windows, registracija predvajalnika in imena paketov za posodobitve.
  Naslovi oken in sporočil so "ApricotPlayer".
- `rust/scripts/build_stable_release.ps1` zgradi `ApricotPlayerSetup.exe` iz
  nespremenjenega Pythonovega `installer/ApricotPlayer.iss` (isti AppId, mapa, opravili,
  registracija, brisanje `_internal`), `ApricotPlayer.zip` in vsote. Privzeti kanal je
  local-only: posodobitve aplikacije samo iz mape v `APRICOT_UPDATE_TEST_FEED`.
- Updater nikoli ne namesti izdaje pod 2.0 (nov test). Lokalna mapa s posodobitvami brez
  `ytdlp-latest.json` yt-dlp še naprej posodablja z GitHuba.

Zamenjava na Urhovem računalniku:

- Kopija Urhovih podatkov na ločenem namizju: izdajna gradnja s Pythonovo identiteto je
  prebrala vse (zgodovina 131, podcasti 4, naročnine 5, playlist z 4 elementi, glavni
  meni z nadaljevanjem seje) in ni spremenila nobene datoteke.
- Varnostna kopija v `%LOCALAPPDATA%\ApricotPlayer-backup\1.0.21-2026-09-30`: vse
  podatkovne datoteke in `components` (brez 2 GB predpomnilnika), preverjeno enake, ter
  namestitveni program 1.0.21. Vrnitev: `restore-python-1.0.21.ps1` v isti mapi
  (`-RestoreData` vrne tudi podatke s 30. 9.).
- Mapa za lokalne posodobitve `%LOCALAPPDATA%\ApricotPlayer-update-feed` in uporabniška
  spremenljivka `APRICOT_UPDATE_TEST_FEED`.
- `ApricotPlayerSetup.exe` 2.0.0-beta.1 je nameščen čez 1.0.21 (Urh je potrdil UAC): vpis za
  odstranitev "ApricotPlayer version 2.0.0-beta.1", `_internal` odstranjen, bližnjica na
  namizju in registracija predvajalnika kažeta na Rust `ApricotPlayer.exe`, ki prestane
  `--qualification-smoke`.
- Stara vgnezdena mapa `C:\Program Files\ApricotPlayer\ApricotPlayer` (Python iz maja,
  480 MB) je ostala, ker je ni ustvarila ta namestitev.
