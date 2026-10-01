# Spotify manifest

Vir zahtev: `docs/SPOTIFY_PLAN.md` (potrjen 30. 9. 2026). Dokazi:
`docs/spotify-p0/P0_EVIDENCE.md` (razdelki v stolpcu Dokaz). Obstoječi
Python-Rust parity gate (`RUST_PARITY_MANIFEST.md`) ostane nespremenjen.

Statusi: `source_verified`, `live_verified`, `implemented`, `accepted`,
`account_unavailable`, `service_unsupported`, `blocked`, `open` (še ni dokaza).
Adapter: **LS** = LibreSpot 0.8.0 knjižnica, **SP** = spclient, **PF** =
pathfinder GraphQL, **AP** = Apricot sama. Faza pove, kdaj se implementira.

| ID | Funkcija | Adapter in metoda | Status P0 | Dokaz | Faza |
| --- | --- | --- | --- | --- | --- |
| S01 | Prijava, refresh, ponovna prijava, odjava | AP PKCE (`apricot-spotify` `oauth.rs`) z LibreSpotovim client ID + LS reusable credentials; discovery kot alternativa | implemented (PKCE prijava v brskalniku s preklicem, rokom, napačnim callbackom in ponovnim poskusom; odjava; zavrnjena prijava se pozabi); open (R01 z NVDA in pravim računom) | 2 | P1 |
| S02 | Več računov in izolacija | AP (ločen cache in credentials po računu) | implemented (seznam računov, izbira aktivnega šele po uspešni povezavi, odjava, odstranitev z mapo računa, DPAPI poverilnice); open (R03 z dvema pravima računoma) | - | P1 |
| S03 | Premium, market, explicit | LS user attributes (`type`, `country`, `filter-explicit-content`) | implemented (Premium ali Free v vrstici računa); ostalo v P2 | 2 | P1 |
| S04 | Predvajanje, pavza, stop, seek | LS Player + mpv most | implemented in live_verified v aplikaciji (Direct link: začetek ~1 s, čas, seek, pavza, nadaljevanje, konec skladbe) | 8 | P2 |
| S05 | Next/previous, EOF | LS Spirc | implemented in live_verified (Ctrl+PageDown/PageUp prek Connect; menjava skladbe ne zažene predvajalnika znova, naslov okna, status in dolžina se zamenjajo, en oglas "Playing") | 5, P3 | P3 |
| S06 | Celoten kontekst | LS Spirc `LoadRequest::from_context_uri`, SP context-resolve | implemented in live_verified (povezava albuma ali playlista v Direct link predvaja kontekst od prve skladbe); izbira iz seznamov v P4 | 4, 6, P3 | P3 |
| S07 | Točna occurrence | LS `PlayingTrack::Uid` (hex `itemId`), uid iz SP | implemented (Play v vrsti začne kontekst na izbrani pojavitvi, live_verified; izbira pojavitve iz playlista v P4) | 5, 6, P3 | P3 |
| S08 | Shuffle, repeat | LS Spirc + remote SetOptions | implemented in live_verified (Shift+S in R za Spotify postavke: shuffle vklop/izklop, repeat izklop/kontekst/skladba s kratkim oglasom) | 5, P3 | P3 |
| S09 | Ročna queue add/remove/reorder/clear | Popravljen LS (SD-1): `Spirc::player_state`, `add_to_queue`, `set_queue` z revizijo | implemented in live_verified (dialog Spotify vrsta, Ctrl+Alt+Shift+Q: dodaj s Ctrl+Shift+Q, premakni, odstrani, počisti ročno dodane, predvajaj izbrano; seznam kaže samo potrjeno stanje); open (stanje po ponovnem zagonu, R07, R08 s telefonom) | 5, P3 | P3 |
| S10 | Connect receiver, transfer | LS Spirc, discovery | implemented in live_verified (naprava "ApricotPlayer (računalnik)", prevzem s telefona: prejšnji medij shrani položaj in se ustavi, fokus ostane) | 5 | P2 |
| S11 | Seznam naprav in oddaljene kontrole | SP cluster `device` map, connect-state ukazi | implemented (dialog Spotify naprave, Ctrl+Alt+Shift+O: ta računalnik prvi, naprava, ki predvaja, označena, Enter prenese predvajanje; live_verified seznam in zavrnitev brez predvajanja ali na že aktivni napravi); open (prenos na telefon in nazaj z Urhovim telefonom) | 5 | P3 |
| S12 | Usklajevanje stanja | SP cluster pubsub na isti seji | implemented (vrsta, skladba, shuffle in repeat sledijo potrjenemu stanju, tudi spremembam z druge naprave); implemented (glasnost: Apricotova glasnost gre v Connect, glasnost, ki jo nastavi telefon, se uporabi brez oglasa, lastni odmev se ne vrne, live_verified brez telefona); open (R08 s telefonom) | 5, P3 | P3 |
| S13 | Autoplay, radio | SP context-resolve autoplay, radio-apollo, inspiredby-mix | live_verified (radio); source_verified (autoplay kontekst) | 3 | P5 |
| S14 | Preload, gapless | LS preload + mpv most brez prekinitve | implemented in live_verified (meja skladbe v istem toku, ura in dolžina se zamenjata ob slišnem prehodu) | 8, P3 | P2 |
| S15 | Kakovost, normalizacija | LS PlayerConfig (96/160/320, normalizacija) | source_verified | 8, 9 | P2 |
| S16 | EQ, boost, speed, pitch, izhod | AP mpv filtri na PCM mostu | implemented (Spotify PCM gre skozi Apricotov libmpv: EQ, hitrost, višina tona, glasnost, izhod); hitrost live_verified, slušni R18 open | 8 | P2 |
| S17 | Medijske informacije | LS metadata, PF `getAlbum`/`getTrack` | implemented (naslov, izvajalec, album, dolžina; format Ogg Vorbis 320 kbps) | 3 | P4 |
| S18 | Resume, bookmarks | AP + vsebinska ura mostu | source_verified (ura) | 8 | P6 |
| S19 | Iskanje vseh tipov | PF `searchDesktop` (+ `searchTracks` ... `searchAudiobooks`) | live_verified | 3 | P4 |
| S20 | Filtri, strani | PF search type-specific queries z offset/limit | source_verified | 3 | P4 |
| S21 | Album, izvajalec, related | PF `getAlbum`, `queryArtistOverview` (top 10, related 20, diskografija) | live_verified | 3 | P4 |
| S22 | Like/Unlike (D17) | PF `addToLibrary`/`removeFromLibrary`, SP `collection/v2/contains` | live_verified; open (uradni klient R13) | 4 | P4 |
| S23 | Shranjeni albumi, izvajalci, oddaje | PF `libraryV3` | live_verified | 4 | P4 |
| S24 | Follow/save vseh tipov | PF `addToLibrary`/`removeFromLibrary` (URI kateregakoli tipa) | source_verified | 4 | P4 |
| S25 | Recently played, top | SP `recently-played/v3` (live_verified); top tracks/artists samo Web API | live_verified (recently); blocked (top prek Web API 429) | 3 | P5 |
| S26 | Home / Made for you | PF `home` | live_verified | 6 | P5 |
| S27 | Daily Mixes | PF `home` + SP playlist `format=daily-mix` | live_verified (6 mixov, vsebina) | 6 | P5 |
| S28 | Discover Weekly, Release Radar, daylist ... | isto, format po formatu | live_verified | 6 | P5 |
| S29 | Browse, kategorije | PF `browseAll` (`browseEndUserIntegration`) | live_verified | 3 | P5 |
| S30 | Radio | SP radio-apollo, inspiredby-mix | live_verified | 3 | P5 |
| S31 | Profili | SP user profile (LS `get_user_profile`) | source_verified | 3 | P5 |
| S32 | Dislike / hide in undo | SP collection set `ban` (globalno), `artistban`, `ignoreinrecs` | live_verified (učinek potrjen v uradnem klientu, razveljavljen) | 7 | P4 |
| S33 | Playlisti, mape | SP rootlist (mape `start-group`/`end-group`), playlist v2 capabilities | live_verified | 3, 4 | P4 |
| S34 | Create | SP `POST /playlist/v2/playlist` + rootlist ADD | live_verified | 4 | P4 |
| S35 | Rename, opis, vidnost, sodelovanje | SP playlist `changes` UPDATE_LIST_ATTRIBUTES, permission endpoints | source_verified | 4 | P4 |
| S36 | Add (tudi dvojniki) | PF `addToPlaylist` | live_verified | 4 | P4 |
| S37 | Remove točne occurrence | PF `removeFromPlaylist` po `uids` | live_verified | 4 | P4 |
| S38 | Move, revision | PF `moveItemsInPlaylist` po `uids`; SP `baseRevision` | live_verified | 4 | P4 |
| S39 | Follow/unfollow playlist | PF library mutations / SP rootlist | live_verified (rootlist add/remove) | 4 | P4 |
| S40 | Cover, share link | PF `fetchPlaylist` images, `sharingInfo` | live_verified (branje) | 4 | P4 |
| S41 | Sort, filter, mape | PF `libraryV3` sort/filter; AP lokalni sort | live_verified (ponujeni sorti) | 4 | P4 |
| S42 | Spremembe s telefona | SP pubsub `hm://playlist/v2/playlist/` | source_verified | 4 | P4 |
| S43 | Oddaje in epizode | PF `queryPodcastEpisodes`, search podcasts/episodes | live_verified (search) | 3 | P6 |
| S44 | Audiobooks | PF `queryBookChapters`, metadata `is_audiobook` | source_verified; open (upravičenost) | 9 | P6 |
| S45 | Lyrics | SP `color-lyrics/v2` (vrstice s časi) | live_verified | 3 | P6 |
| S46 | Transcript, poglavja | SP `transcript-read-along/v2` | open | 9 | P6 |
| S47 | Cover, credits, explicit | PF `getAlbum` (copyright, coverArt), LS metadata | live_verified | 3 | P4 |
| S48 | Preview | LS `get_audio_preview` | source_verified | 1 | P6 |
| S49 | Lokalne datoteke | LS local_file_directories; Connect jih zavrne | source_verified (omejitev) | 9 | P6 |
| S50 | URI, open.spotify.com URL | AP parser | implemented (`SpotifyRef`; Direct link predvaja povezave skladb in epizod, druge vrste v naslednjih fazah) | - | P1 |
| S51 | Favorites, zgodovina, mešani playlisti | AP | open | - | P6 |
| S52 | Background player, tray, media keys | AP | open | - | P2 |
| S53 | Cache, offline, čiščenje | LS Cache (limit), AP ločitev credentials | source_verified | 1 | P6 |
| S54 | Diagnostika brez skrivnosti | AP (redakcija kot v evidenci P0) | source_verified (orodje) | uvod | P6 |
| S55 | Tipkovnica, NVDA, meniji, lokalizacija | AP | implemented za hub, račune in prijavo (dve dejanji z bližnjicama, kontekstni meni, Action Finder, 27 jezikov); ostalo po fazah | - | P1-P7 |

## Meje (razdelek 3.5 plana)

Offline prenos: `service_unsupported` (cache ni prenos). Lossless:
`service_unsupported` v LibreSpot 0.8.0. Crossfade: ni v knjižnici, potrebna
odločitev. Canvas: podatki obstajajo, brez vsebine za bralnik zaslona. Smart
Shuffle: pathfinder query obstaja, ni preizkušeno. DJ, Jam, private session,
Wrapped: `open`, brez znanega vmesnika v P0.

## Odločitve med izvedbo

| ID | Datum | Odločitev |
| --- | --- | --- |
| SD-1 | 30. 9. 2026 | Urh je izbral možnost a: potrjeno Connect stanje (vrsta z `uid` in `queue_revision`) izpostavi ozek lokalni popravek `librespot-connect` prek `[patch.crates-io]`, hkrati se pripravi upstream predlog. Izvedba v P3. |
| SD-2 | 30. 9. 2026 | `MediaSource::Spotify` se doda v P2 skupaj s prvim predvajanjem; P1 uvede samo trajno referenco `SpotifyRef`, da se Python podatki ne spremenijo brez potrebe. |
| SD-3 | 30. 9. 2026 | Hub prikazuje samo delujoče vnose (P1: Prijava, Spotify računi); vsaka faza doda svoje vnose, brez neaktivnih vrstic. |
| SD-4 | 1. 10. 2026 | En sam glasnostni člen: Connect glasnost se hrani, uporablja se Apricotova (mpv). Sinhronizacija glasnosti s telefonom v P3. |
| SD-5 | 1. 10. 2026 | rustls dobi izrecno izbranega ponudnika aws-lc-rs ob zagonu Spotify storitve (v drevesu sta ring in aws-lc-rs). |
| SD-6 | 1. 10. 2026 | Spotify vrsta je modalni dialog kot Apricotova vrsta (Ctrl+Alt+Q), ne zaslon glavnega okna, zato je odprtje iz predvajalnika ne ustavi. Urejanja nosijo UID pojavitve; lastni `set_queue` ohrani UID-je (popravek `replace_next_tracks`). |
| SD-7 | 1. 10. 2026 | Ctrl+PageUp/PageDown, Shift+S in R pri Spotify postavki gredo v Connect, ne v Apricotovo zaporedje. Zaslon naprav (S11) je naslednji korak P3b. |
| SD-8 | 1. 10. 2026 | P3b: naprave so modalni dialog kot vrsta. Popravek `librespot-connect` objavi seznam naprav iz vsakega prejetega clusterja in samo glasnost, ki jo nastavi druga naprava, zato Apricot sledi telefonu brez zanke; telefon nad 100 % (Apricotov boost) vidi 100 %. |
