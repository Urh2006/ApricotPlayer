# Spotify P0: dokazna evidenca

Datum: 30. 9. 2026. Osnova: `rust-2.0` HEAD `9b4aa2c` (2.0.0-beta.3), plan
`docs/SPOTIFY_PLAN.md`. Orodje: `rust/tools/spotify-p0` (ločen Cargo workspace,
ni del produkta). Račun: Premium, market SI, en račun (Urhov, prijava v
njegovem brskalniku). Evidenca ne vsebuje tokenov, uporabniškega imena, e-pošte
ali naslovov; poverilnice so bile samo v `%LOCALAPPDATA%\ApricotPlayer-spotify-p0`
in se na koncu P0 izbrišejo (`spotify-p0 forget`).

Oznake: **SV** = source_verified, **LV** = live_verified, **OPEN** = še ni
dokazano, **BLK** = blokada, ki potrebuje odločitev.

## 1. Zaklenjena različica in knjižnica

| Postavka | Odločitev in dokaz |
| --- | --- |
| Crate | `librespot` 0.8.0 (crates.io, 2025-11-10, MIT, MSRV 1.85). Posamezni crati `librespot-{core,oauth,discovery,playback,connect,metadata,protocol}` `=0.8.0`. |
| Feature flags | `default-features = false`, `native-tls` (SChannel na Windows), discovery `with-libmdns`. Brez `rodio` backenda: zvok gre prek lastnega `Sink` v libmpv. |
| Znana napaka gradnje | 0.8.0 se ne zgradi z `vergen 9.1`; lock na `vergen 9.0.6` (upstream popravek 5440c4d na `dev`). Zapisano v `rust/tools/spotify-p0/Cargo.lock`. |
| `dev` veja | 21 commitov za 0.8.0. Relevantni: `Spirc::add_to_queue` (87d37c3), `Spirc::clear_queue` (a1b66d3), dogodek `SetQueue` (33bf3a7), device authorization flow (1599145), CDN fallback (db1ef7a). Priporočilo: ostati na 0.8.0 in queue urejati prek Connect ukaznega API (glej 5), dokler upstream ne izda naslednje različice. |
| Gradnja | `cargo build` zelen na Rust 1.98, Windows 11. |

## 2. Prijava in distribucija (P0 dokaz 1)

| Dokaz | Rezultat |
| --- | --- |
| Tok | OAuth authorization code + PKCE (S256) + state v sistemskem brskalniku, loopback `http://127.0.0.1:5588/login`. **LV**: prijava uspela, 26 scopes, refresh token prisoten, seja vzpostavljena, reusable credentials shranjene. |
| Client ID | LibreSpot vedno uporabi vgrajeni Client ID uradnega Spotify namiznega klienta (`librespot-core` `config.rs`, `KEYMASTER_CLIENT_ID`). Apricot ga ne kopira, uporablja `SessionConfig::default()`. Uporabnik ne potrebuje developer računa (D03 izpolnjen). |
| Ponovni zagon | **LV**: vsi nadaljnji zagoni orodja (več deset) so se povezali s shranjenimi poverilnicami v 0,6 do 1,3 s brez brskalnika. |
| Alternativa | Zeroconf discovery (telefon izbere napravo in preda poverilnice) je v kodi (`spotify-p0 discover`), **SV**. Device authorization flow je samo na `dev`. |
| Lastna developer aplikacija | Zavrnjena kot pot za D03: od 11. 2. 2026 Development Mode zahteva Premium lastnika, največ 5 uporabnikov na aplikacijo, playlist vsebina samo za lastne playliste, iskanje največ 10 zadetkov. Vir: Spotify February 2026 migration guide. |
| Preklic, odjava | **OPEN** za P1: preklic brskalnika, napačen callback, odjava in odstranitev računa se preverijo z UI. |
| Tveganje | Uporaba notranjih vmesnikov in identitete uradnega klienta je neuradna. Upstream LibreSpot jo uporablja; sprememba s strani Spotifyja lahko pokvari prijavo ali posamezne klice. Zato ima vsak adapter contract fixture in jasno napako. |

## 3. Izbira adapterja

| Vmesnik | Rezultat z LibreSpot tokenom (login5) |
| --- | --- |
| Uradni Web API `api.spotify.com` | **BLK za to pot**: vseh 32 preizkušenih klicev vrne 429 z `Retry-After` 35 do 47 s, tudi ponovno po čakanju. Keymaster token prek Mercury vrne 403 "Invalid request". Web API se zato ne uporablja. |
| spclient (`*.spclient.spotify.com`) | **LV**: rootlist (136 elementov, mape), playlist v2 (revision, capabilities, item uid), context-resolve (Liked Songs: 13352 skladb v enem kontekstu), collection v2 paging/contains, radio (apollo in inspiredby), lyrics, recently played. Autoplay kontekst je samo **SV** (klic v P0 ni bil izveden). |
| Pathfinder GraphQL (`api-partner.spotify.com`) | **LV**: `searchDesktop`, `home`, `libraryV3`, `fetchPlaylist`, `getAlbum`, `queryArtistOverview`, `browseAll`, `areEntitiesInLibrary`, `addToLibrary`, `removeFromLibrary`, `canvas`. Persisted query hashi so javna koda spletnega predvajalnika (ne skrivnost); orodje jih prebere iz `open.spotifycdn.com` bundlov. |

Posledica za arhitekturo: `apricot-spotify` ima adapter `internal` (spclient +
pathfinder) z enim mestom za hash register, contract fixtures in razločevanjem
"schema drift" od "prazne zbirke". Hash register se ob napaki
`PersistedQueryNotFound` osveži iz bundlov; brez tega se funkcija oglasi kot
začasno nedosegljiva, ne kot prazna.

## 4. Knjižnica in mutacije (P0 dokaz 2)

| Operacija | Pot | Rezultat |
| --- | --- | --- |
| Like (D17) | pathfinder `addToLibrary` `libraryItemUris` | **LV**: skladba ni bila v knjižnici, po Like `collection/v2/contains` = true in pathfinder `saved` = true. |
| Unlike | pathfinder `removeFromLibrary` | **LV**: po Unlike `contains` = false. Stanje povrnjeno. |
| Uradni klient | ročni pregled | **OPEN** za P4 (R13): Urh preveri v uradnem klientu v obeh smereh. |
| Ustvari playlist | spclient `POST /playlist/v2/playlist` (Delta z imenom) | **LV**: vrne URI in revision. |
| Dodaj v knjižnico | spclient `POST /playlist/v2/user/{u}/rootlist/changes` ADD | **LV**. |
| Odstrani iz knjižnice (Spotifyjev "Delete") | rootlist REM z `baseRevision` in `itemsAsKey` | **LV**: odstranjen točno testni playlist, ostali nespremenjeni. |
| Dodaj, odstrani, premakni skladbo | pathfinder `addToPlaylist` (`playlistItemUris`, `newPosition`), `removeFromPlaylist` (`uids`), `moveItemsInPlaylist` (`uids`, `TOP_OF_PLAYLIST`/`BOTTOM_OF_PLAYLIST`/`BEFORE_UID`/`AFTER_UID`) | **LV** (z Urhovim izrecnim dovoljenjem): dodano A, B, A; vsaka pojavitev ima svoj `uid`; odstranjena samo druga pojavitev A (prva ostane); B premaknjen na vrh; `uid` ostanejo stabilni med urejanji; ponovna odstranitev že odstranjenega `uid` je brez učinka (200, nič se ne izbriše), zato Apricot po vsaki mutaciji prebere potrjeno stanje. Testni playlist nato odstranjen iz knjižnice z `baseRevision`. |
| Sočasno urejanje | spclient `changes` z `baseRevision`, playlist pubsub `hm://playlist/v2/playlist/` | **SV**; rootlist REM z revision je **LV**. |
| Albumi, izvajalci, oddaje | `libraryV3` | **LV**: 11155 elementov, tipi Playlist, Album, Artist, PseudoPlaylist (Liked Songs), filtri in sort vrstni redi vrnjeni. |

## 5. Queue in Connect (P0 dokaz 4)

| Postavka | Stanje |
| --- | --- |
| Izbira occurrence | `LoadRequestOptions.playing_track = PlayingTrack::Uid` (**SV**); uid je vrnjen v playlist v2 in context-resolve (50/50 skladb Spotify playlista ima uid, **LV**). |
| Oddaljeni ukazi, ki jih Spirc sprejme | Transfer, Play, Pause, SeekTo, SetShufflingContext, SetRepeatingTrack, SetRepeatingContext, AddToQueue, SetQueue, SetOptions, UpdateContext, SkipNext, SkipPrev, Resume (`core/src/dealer/protocol/request.rs`, **SV**). |
| Dodaj, odstrani, premakni, počisti ročno vrsto | **LV**: `add_to_queue` in `set_queue` prek `connect-state/v1/player/command/from/{naprava}/to/{aktivna}`. Dvojnik dobi svoj `uid` (q5, q7). Premik (4. na 1. mesto), odstranitev točno ene pojavitve in čiščenje (context in autoplay ostaneta) so potrjeni v cluster posodobitvi. Vrsta ima prednost pred kontekstom (po koncu skladbe je naslednja začela `provider=queue`). |
| Nevarnost zastarelega stanja | **LV**: `set_queue` zamenja celotno vrsto in LibreSpot ne preveri `queue_revision`. Ukaz, zgrajen iz zastarelega stanja, je izbrisal dve skladbi preveč. Aktivna LibreSpot naprava ne dobi lastnih cluster posodobitev, 0.8.0 pa nima javnega branja stanja (`dev` ima dogodek `SetQueue`, vendar brez `uid`). Odločitev: ozka upstream razširitev ali lokalni patch, ki izpostavi potrjeni `PlayerState` (z `uid` in `queue_revision`) po vsaki spremembi; do takrat urejanje vrste na lastni napravi ni varno. Uid-ji vrste se po `set_queue` preštevilčijo, zato se tarča vedno določi iz zadnjega potrjenega stanja. |
| Potrjeno stanje | Apricot posluša `hm://connect-state/v1/cluster` na isti seji (dealer dovoli več poslušalcev) in prikazuje samo potrjeni `PlayerState` (next/prev s provider `queue` ali `context`, `queue_revision`, opcije). Nobene lokalne kopije vrste (D07). |
| Transfer telefon -> Apricot | **LV**: Urh je predvajanje s telefona prenesel na "ApricotPlayer P0"; cluster `active_is_me=true`, kontekst se je nadaljeval, po koncu konteksta autoplay. Telefon napravo prikaže pod "on other networks", ker ni objavljena z Zeroconf v LAN; prenos gre prek oblaka. Prenos nazaj na telefon **OPEN** (ni nujen za odločitev). |
| Oddaljeno upravljanje | **LV**: druga (neaktivna) naprava je z istim ukaznim API-jem urejala vrsto aktivne naprave (osnova za S11). |
| Identiteta naprave | `SessionConfig::default()` ustvari naključni `device_id` ob vsakem zagonu; Apricot mora `device_id` hraniti na instalacijo, ime naprave pa naj vsebuje ime računalnika. |

## 6. Personalizacija in Daily Mixes (P0 dokaz 3)

| Postavka | Rezultat |
| --- | --- |
| Odkrivanje | pathfinder `home` (`homeEndUserIntegration: INTEGRATION_WEB_PLAYER`), **LV**: 21 sekcij, 156 playlistov. Sekcije imajo stabilne `spotify:section:` URI-je. |
| Identiteta mixov | Vsak playlist ima atribut `format` (spclient playlist v2). Na Home tega računa: `daily-mix` 6, `artist-mix-reader` 32, `inspiredby-mix` 24, `descripto` 17, `topic-mix` 15, `discover-weekly` 1, `release-radar` 1, `daylist` 1, `on-repeat` 1, ter editorialni formati. Osebni imajo `madeFor.username`. Identifikacija torej ni po naslovu ali globalnem ID-ju. |
| Vsebina | Spotify-owned playlist: 50 skladb z uid prek context-resolve in playlist v2 (**LV**). |
| Začetek iz izbrane skladbe | **LV**: `Spirc::load` Daily Mixa s `PlayingTrack::Uid` pete pojavitve je začel peto skladbo (`prev=4`), naslednja je šesta. Uid v Connect stanju je hex zapis `itemId` iz playlist v2. Track ID v predvajanju se lahko razlikuje od ID-ja v playlistu (market relinking, SI), zato Apricot occurrence vedno ujema po `uid`, nikoli po track ID-ju. Predvajanje je bilo takoj ustavljeno. |
| Drugi račun | **OPEN**: R15 zahteva dva običajna računa. |

## 7. Dislike (P0 dokaz 6)

| Postavka | Stanje |
| --- | --- |
| Spotifyjeva dejanja | "Hide song" v kontekstu (`addContextTrackBan` / `removeContextTrackBan`), "Don't play this artist" (collection set `artistban`), "Exclude from taste profile" (collection set `ignoreinrecs`). |
| Kje velja hide | Samo v personaliziranih kontekstih s formati `artistsets`, `artist-mix-reader`, `blend`, `daily-mix`, `daylist`, `discover-weekly`, `descripto`, `inspiredby-mix`, `on-repeat`, `release-radar`, `repeat-rewind`, `topic-mix` ter v autoplay (seznam iz kode spletnega klienta). |
| Vmesnik | Spletni predvajalnik ima `canBan: false`, namizni klient pa ga izvaja. Kandidat: collection set `ban` z `context_uri` (polje 4 v `collection2v2.proto` `CollectionItem`); branje seta `ban` deluje (**LV**, 0 elementov). |
| Zapis | **LV** (z Urhovim dovoljenjem): `collection/v2/write` set `ban` z eno skladbo iz Daily Mix 6 in `context_uri`; strežnik je shranil zapis brez konteksta (`{uri, added_at}`), torej gre za splošno skrito skladbo, ne samo v mixu. |
| Dokaz učinka | **LV**: Urh je v uradnem klientu potrdil, da je skladba v Daily Mix 6 prikazana kot skrita. Razkritje z `is_removed` je set `ban` vrnilo na 0 elementov. D04 je izvedljiv z dejanskim Spotifyjevim dejanjem; ker Spotify zapis hrani brez konteksta, Apricot dejanje poimenuje "Skrij skladbo" in ga ponudi v personaliziranih kontekstih, kot uradni klient. |

## 8. Zvok (P0 dokaz 5)

Merjeno na tem računalniku, 160 kbps (privzeto), `ao=null` (brez zvoka).

| Meritev | Rezultat |
| --- | --- |
| Dekodiranje Premium toka | **LV**: PCM 44,1 kHz stereo, market relinking deluje (drug track ID v SI). |
| Čas do prvega zvoka | 1,12 s od `load` s capture sinkom; 0,79 s skozi mpv most (seja že povezana); povezava seje 0,6 do 1,3 s. |
| Most | LibreSpot `Sink` -> 200 ms ring (backpressure) -> libmpv `mpv_stream_cb_add_ro` + `demuxer=rawaudio` -> obstoječi mpv filtri. Brez zapisa na disk. |
| Ograja seek in skladb | Sink ima lasten `PlayerEventChannel` in ga prazni s `try_recv` pred vsakim zapisom. LibreSpot pošlje `Seeked`/`Playing` na player threadu pred prvim novim paketom, zato je ograja točna brez branja internih polj. Ob ograji se odpre nova generacija, stara se zapre (brez starega PCM). |
| Seek | 243 ms od ukaza do novega zvoka v mpv; nova generacija z bazo 59 998 ms. |
| Ura | vsebinski čas = baza generacije + mpv `time-pos` (medijski čas). Pri hitrosti 2,0 se `time-pos` premika 2 s na sekundo, LibreSpot dekodira sorazmerno hitreje (backpressure). LibreSpotov poročani položaj prehiteva zvok za 0,4 do 0,9 s (ring + mpv buffer); Connect zato dobiva položaj z zamikom pod 1 s. |
| Hitrost, višina tona, EQ | **LV**: `scaletempo2` (audio_chain Mpv), `rubberband` pitch 1,12 in `lavfi equalizer` sprejeti brez napak med predvajanjem. Slušna kakovost **OPEN** (R18, Urh). |
| Pavza | LibreSpot `pause` + mpv `pause`: čas stoji, nadaljevanje brez skoka. |
| Gapless | "Speak to Me" -> "Breathe": preload ob `TimeToPreloadNextTrack`, `load` ob `EndOfTrack`; izhod ni nikoli ostal brez podatkov (najmanjša rezerva 496 ms od 500), sink se ni ustavil. |
| Normalizacija | LibreSpot normalizacija je izklopljena privzeto; Apricot uporabi eno plast gaina (odločitev v P2). |

Izbrana strategija: kandidat 1 iz plana (LibreSpot sink -> libmpv). Kandidat 2
(lasten DSP) ni potreben.

## 9. Meje iz razdelka 3.5

| Zmožnost | Ugotovitev |
| --- | --- |
| Offline prenos, izvoz | LibreSpot cache je šifriran avdio cache, ne uporabniški prenos. Download/Copy stream URL/Save edit copy za Spotify ostanejo nedostopni. |
| Lossless | `Bitrate` v 0.8.0 pozna samo 96/160/320 (Vorbis); lossless ni podprt. |
| Crossfade | Ni v LibreSpotu; lokalno bi ga lahko naredil mpv, ni del obsega brez odločitve. |
| Canvas | pathfinder `canvas` vrne podatke (**LV**); to je video zanka, za NVDA nima vsebine. |
| Smart Shuffle | pathfinder `smartShuffle` obstaja (**SV**), ni preizkušeno. |
| DJ, Jam, private session, Wrapped | Ni v LibreSpotu; ni dokazov o vmesniku. **OPEN**, se dokumentira v manifestu. |
| Lokalne datoteke | `local_file_directories` v PlayerConfig; Connect jih zavrne ("playback of local files is not supported" v `connect/src/state.rs`). |
| Transcript | spclient `transcript-read-along/v2` za naključno epizodo 404; **OPEN** z epizodo, ki ima prepis. |
| Audiobooks | metadata ima `is_audiobook`/`is_audiobook_chapter`, pathfinder `queryBookChapters`; upravičenost **OPEN**. |

## 10. Odprto za zaključek P0

1. Odločitev o izpostavitvi potrjenega Connect stanja (glej 5).

Poverilnice in surovi odgovori so bili 30. 9. 2026 izbrisani (`spotify-p0 forget`). Vse testne spremembe v računu so razveljavljene: Like, testna playlista, vrsta, skrita skladba.

## 11. Izbrane odločitve za P1 in naprej

| Tema | Odločitev |
| --- | --- |
| Različica | LibreSpot 0.8.0, `default-features = false`, `native-tls`, lock `vergen 9.0.6`. |
| Adapter | `apricot-spotify` z notranjim adapterjem (spclient + pathfinder); Web API se ne uporablja. Hash register persisted queries z osvežitvijo iz bundlov in contract fixtures. |
| Zvok | LibreSpot `Sink` -> 200 ms ring -> libmpv `stream_cb` rawaudio -> obstoječa `audio_chain`. Ograja prek lastnega `PlayerEventChannel` v sinku. Vsebinska ura = baza generacije + `time-pos`. Prehod med skladbami ostane v isti generaciji; meja se zabeleži ob `Playing` z novim `play_request_id`. |
| Occurrence | Vedno `uid` (playlist v2 `itemId` kot hex, pathfinder `uid`), nikoli track ID ali indeks. |
| Connect stanje | Priporočilo: ozek lokalni patch `librespot-connect` (Cargo `[patch.crates-io]`, vendorirana 0.8.0 z eno javno metodo ali dogodkom za potrjeni `PlayerState` z `uid` in `queue_revision`) in hkrati upstream PR. Alternativa brez patcha je druga, skrita dealer seja kot opazovalec, kar je bolj krhko. Odločitev je Urhova. |
| Identiteta naprave | `device_id` trajen na instalacijo, ime naprave z imenom računalnika. |
