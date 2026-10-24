# Preverba 26 GPT-jevih točk (Rust 2.0.0-beta.1, commit 782bf61)

Datum: 30. 9. 2026. Vsako točko sem preveril v dejanski kodi obeh verzij, predvajalniške
točke pa še z dejansko priloženo `libmpv-2.dll` in kratkim WAV posnetkom.

Oznake: **potrjeno** (napaka obstaja, kot je opisana), **delno** (napaka obstaja, opis
ni povsem točen), **tudi v Pythonu** (enaka napaka je v Pythonu 1.0.21, zato jo Rust sme
popraviti šele po tvoji odobritvi).

## Povzetek

- Popravljeno z regresijskimi testi: 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  17, 18, 19, 21, 23 ter padajoči RSS test.
- Potrjeno, a ostaja kot nova enota: 20 (predpomnilnik naslovov tokov in vnaprejšnje
  razreševanje naslednjega elementa). To je manjkajoča funkcija, ne majhen popravek.
- Čaka na tvojo odločitev (P-11 do P-15): 1, 22, 24, 25, 26.
- Zavrnjena ni nobena točka. Pri 16 in 22 je bil opis delno netočen (glej spodaj).

## Točka za točko

| # | Ocena | Dokaz | Stanje |
|---|---|---|---|
| 1 | tudi v Pythonu | Izolirana ponovitev v ločeni mapi: zaklenjena ciljna datoteka, rollback pusti delno novo datoteko in izbriše backup. Pythonov skript (`updater.py`) ima isto logiko. Del o delnem kopiranju mape v Rustu ne povzroči izgube, pusti le odvečne nove datoteke. | P-11 |
| 2 | potrjeno | libmpv s `keep-open=yes` na koncu pošlje le `pause=yes`, `END_FILE` ne pride. | popravljeno |
| 3 | potrjeno | libmpv pri začetnem premoru pošlje `Paused(true)` in nato `FILE_LOADED`; seja je prešla v Playing. | popravljeno |
| 4 | potrjeno | Po `pause=no` na koncu libmpv ne pošlje ničesar, položaj ostane na koncu. | popravljeno |
| 5 | potrjeno | `WM_DESTROY` ni shranil položaja. | popravljeno |
| 6 | potrjeno | `validate_media_url` je zavrnil vse razen YouTuba in SoundClouda. | popravljeno |
| 7 | potrjeno | `MediaItem::from_direct_link` je dovolil le http in https. | popravljeno |
| 8 | potrjeno | Kanal 128, `try_send` zavrže, UI med modalnim oknom ne bere. | popravljeno |
| 9 | potrjeno | libmpv: manjkajoča datoteka, 300 ms premora, nato veljavna: stara napaka je prišla kot napaka novega posnetka. | popravljeno |
| 10 | potrjeno | `pending_queued_start` se je počistil pred Direct fallbackom. | popravljeno |
| 11 | potrjeno | Action Finder ni obravnaval `IDOK`, queue ne `IDOK` in `IDCANCEL`. | popravljeno |
| 12 | potrjeno | `handle_shortcut_message` je tekel pred poljem za zajem; poleg tega je `IsDialogMessageW` Enter in Escape v polju spremenil v `IDOK`/`IDCANCEL`. | popravljeno |
| 13 | potrjeno | `ShortcutChord::parse` je vzel le prvo alternativo. | popravljeno |
| 14 | potrjeno | Konflikt se je preverjal z nerazčlenjenim besedilom. | popravljeno |
| 15 | potrjeno | `is_plain_text_input` je Shift+črko štel za bližnjico. | popravljeno |
| 16 | delno | Omejitev 144p je v nastavitvah **prenosov**, ne predvajanja, kot piše GPT. | popravljeno |
| 17 | potrjeno | `ClipExportRequest` ni imel HTTP glav. | popravljeno |
| 18 | potrjeno | Za spletni video brez lokalne poti je bila vedno pripona `.mp4`. | popravljeno |
| 19 | potrjeno | `upsert_front` je zavrnil playliste in kanale. | popravljeno |
| 20 | potrjeno | Nastavitve obstajajo, uporabe ni. Python ima trajni `stream_url_cache.json` in prefetch. | nova enota |
| 21 | potrjeno | Python priloži `node.exe` in ga poda yt-dlp; Rust ga ni. Manjkal je tudi ponovni poskus s klientom `web_safari`. | popravljeno |
| 22 | delno | Res, `player_command` nima učinka. Vendar je Rust namerno libmpv v procesu, zunanjega `mpv.exe` ne more uporabiti kot Python. | P-12 |
| 23 | potrjeno | `apply_refreshes` je po osvežitvi ponovno razvrstil feede. | popravljeno |
| 24 | tudi v Pythonu | `replace_converted_original` v obeh verzijah prepiše obstoječi `song.wav` in izbriše `song.mp3`. | P-13 |
| 25 | tudi v Pythonu | Zahteve nimajo generacije; Rust sicer preveri zaslon, ne loči pa Recent movies od Recent TV. | P-14 |
| 26 | tudi v Pythonu | Zaključek priprave ne preveri, ali je zahteva še aktualna. | P-15 |
| RSS test | potrjeno | Na Windows sprejeti socket podeduje neblokirajoč način. | popravljeno |

## Kaj je spremenjeno

- **Konec predvajanja (2, 4).** libmpv engine opazuje `eof-reached`. Premor, ki ga mpv
  sam nastavi na koncu, poroča kot en sam dogodek Ended (ne kot "Paused"), zato steče
  samodejno naslednji, sorodni, označevanje podcasta in "Playback finished". Konec se
  poroča šele, ko zvok res izzveni (eof-reached mpv nastavi okoli 0,4 s prej). Play ali
  Space na koncu, tako kot Python `restart_current_playback`, skoči na začetek, predvaja
  in oglasi "Playback restarted from the beginning." brez dodatnega "Playing". Če si se
  po koncu premaknil nazaj, Play le nadaljuje (Python `player_should_restart_from_end`).
  Gumb Play/Pause po koncu kaže Play.
- **Začetni premor (3).** Started ne prepiše premora, Space nadaljuje.
- **Stari dogodki (9).** Engine si zapomni vnos `loadfile` in zavrže dogodke zamenjanega
  posnetka.
- **Modalna okna (8).** Kadar je kanal poln, runtime pomembne dogodke hrani v vrsti in jih
  pošlje po zaprtju okna; zavrže le vmesne položaje (zadnji položaj je vedno na voljo).
- **Izhod (5).** Ob zaprtju aplikacije se shrani položaj, kot v Pythonu.
- **Direct link (6, 7, 10).** Vsak http(s) naslov gre skozi yt-dlp (generična ekstrakcija,
  HTTP glave za mpv), ob napaki ostane fallback na mpv. RTSP, RTMP in MMS so sprejeti.
  Element iz čakalne vrste se po uspešnem fallbacku odstrani.
- **Dialogi (11, 12).** Enter v Action Finderju odpre izbrano dejanje, Enter in Escape v
  playback queue predvajata oziroma zapreta. Polje za zajem bližnjice zajame vsako
  kombinacijo, tudi Enter in Escape, le Tab ga zapusti; globalne bližnjice med zajemom ne
  tečejo.
- **Bližnjice (13, 14, 15).** Vse alternative `A | B` delujejo; konflikt se preverja v
  kanonični obliki; Shift+črka v besedilnem polju je pisanje.
- **Prenosi in izrezi (16, 17, 18, 19).** Višina 0 pomeni brez omejitve; izvoz izseka poda
  FFmpeg HTTP glave in uporabi pripono toka (`ext`), pri podcastu `.m4a`; preneseni
  playlisti in kanali se zapišejo v zgodovino.
- **YouTube (21).** Paket vsebuje `node\node.exe` (enak kot v Pythonu), yt-dlp ga dobi z
  `--js-runtimes`; ob napakah, kot je "nsig extraction failed", sledi en ponovni poskus s
  klientom `web_safari`.
- **RSS (23).** Osvežitev zamenja feed na istem mestu.

Znana omejitev: dialogi za poglavja, prepis in komentarje še vedno upoštevajo le prvo
alternativo bližnjic `open_selected` in `player_back`.

## Odločitve za tebe (odgovori z da ali ne)

- **P-11 (točka 1):** Ali naj Rust updater ob neuspeli obnovitvi obdrži varnostno kopijo
  in to zapiše v dnevnik, namesto da jo izbriše? Python jo izbriše. Priporočam da.
- **P-12 (točka 22):** Ali naj Rust uporabi mapo iz nastavitve `player_command`, kadar je
  v njej `libmpv-2.dll`, sicer pa priloženi mpv? Priporočam ne, ker zunanja libmpv
  druge verzije lahko pokvari predvajanje, priloženi mpv pa vedno deluje.
- **P-13 (točka 24):** Ali naj pretvorba z zamenjavo originalov ne prepiše druge obstoječe
  datoteke z istim imenom, ampak zapiše "song (2).wav"? Priporočam da.
- **P-14 (točka 25):** Ali naj Rust zavrže pozne AudioVault rezultate starejšega iskanja
  ali drugega Recent pogleda? Priporočam da.
- **P-15 (točka 26):** Ali naj Rust ne zažene pozno pripravljenega AudioVault posnetka,
  če si medtem izbral drugega ali zapustil AudioVault? Priporočam da.
- **Točka 20:** Ali naj predpomnilnik tokov in prefetch naredim kot naslednjo enoto (E21)?
