# Celovit plan Spotify podpore v ApricotPlayerju 2.0

Datum: 30. 9. 2026.

Status: celoten izvedbeni plan je uporabnik potrdil 30. 9. 2026 z odgovorom
»ja potrdim«. Potrditev zajema produktne odločitve iz `/grill-me`, arhitekturo,
razpored zaslonov, predlagane privzete bližnjice, faze in acceptance pogoje.
Ponovna potrditev iste zasnove ni potrebna. Ta dokument ni dokaz izvedljivosti ali
implementacije. V tem koraku niso bili izvedeni prijava, posegi v Spotify račun,
prototip predvajanja, spremembe aplikacijske kode ali objava.

Osnova: `rust-2.0`, HEAD `782bf61`, Cargo `2.0.0-beta.1`, Python `1.0.21`.
Med pripravo so v delovni kopiji prisotni Claudovi popravki rewritea. Ti ostanejo
nedotaknjeni; tehnične sklice je pred izvedbo treba preveriti na novem stanju.

Ta načrt je nov, uporabniško zahtevan obseg za prvo javno Windows Rust 2.0.
Stara izključitev Spotifyja v `RUST_REWRITE_PLAN.md`, `RUST_PARITY_MANIFEST.md`
in `BACKLOG.md` ne predstavlja več uporabnikovega želenega obsega. Med
implementacijo je treba te dokumente ozko uskladiti in dodati Spotify manifest;
obstoječe Python–Rust acceptance pogodbe se ne sme zmanjšati.

## 1. Potrjene produktne odločitve

| ID | Odločitev |
| --- | --- |
| D01 | Spotify je običajna, privzeto vidna možnost glavnega menija. Enter odpre seznam Spotify funkcij v obstoječem Apricotovem slogu. |
| D02 | Vključiti vse preverjeno podprte uporabniške zmožnosti LibreSpota ter dodatne Spotify vmesnike za preostali obseg. Ne omejiti integracije na Connect receiver. |
| D03 | Uporabnik se sam prijavi s svojim računom. Ne potrebuje developer računa, Client ID-ja ali Client Secret-a. |
| D04 | Dislike pomeni dejansko Spotifyjevo negativno dejanje, samo kjer je podprto. Brez lokalne nadomestne blacklist. Odstranitev iz Liked Songs je ločeno dejanje. |
| D05 | Spotify shuffle in repeat sta ločena od drugih virov, upravljanje pa uporablja Apricotove vzorce. |
| D06 | Spotify queue je ločena od splošne Apricotove queue. |
| D07 | Spotify queue in kontekst sta usklajena z aktivno Connect sejo; prikaz mora slediti dejansko potrjenemu stanju. |
| D08 | Izbira Apricota kot Connect naprave na telefonu prevzame lokalno predvajanje. Prejšnji medij se ustavi in njegov položaj shrani, fokus pa ostane. |
| D09 | Lokalno Spotify predvajanje podpira celoten Apricotov nabor zvočnih kontrol: EQ, boost, hitrost in višino tona ter druge smiselne lokalne player funkcije. |
| D10 | Podprtih je več shranjenih računov. Seja, Spotify queue, knjižnični cache in Spotify nastavitve so ločeni po računu. |
| D11 | Spotify autoplay je ločena nastavitev. Ob koncu konteksta lahko nadaljuje s Spotifyjevimi priporočili, če jih vmesnik zagotavlja. |
| D12 | Spotify reference sodelujejo v Apricotovih Favorites, zgodovini, bookmarkih in uporabniških playlistih. Apricot Favorite ne spremeni Spotify Liked Songs. |
| D13 | Celoten dogovorjeni Spotify obseg in Python–Rust pariteta sta pogoja prve javne Rust 2.0. Faze pomenijo vrstni red razvoja, ne odloga zahtev v naslednji release. |
| D14 | Ob običajnem izboru se glasba začne pri 0:00; govorna vsebina nadaljuje s shranjenega položaja. Namenski Resume last session obnovi položaj tudi pri glasbi. |
| D15 | Posamezna nedostopna skladba se preskoči z uporabnim obvestilom, brez brisanja iz playlista. Napaka računa ali povezave ustavi predvajanje. |
| D16 | Enter na skladbi drugega playlista jo začne takoj in ohrani ročno queue. Nato se predvaja queue, nato novi playlist po njegovem shuffle/repeat. |
| D17 | Like skladbe pomeni dejanski Spotify Like: skladbo doda v Liked Songs aktivnega računa, enako kot neposredno v Spotifyju. Unlike jo odstrani iz Liked Songs; nobeno od teh dejanj ni dislike ali Apricot Favorite. |

## 2. Dejstva, tehnične meje in začetna preverjanja

### 2.1 Kaj je trenutno preverjeno

Upstream LibreSpot opisuje Premium predvajanje in Spotify Connect. Free playback
ni njegov podprt cilj. Aplikacija mora to jasno sporočiti in ne sme obljubljati
predvajanja Free računa. [LibreSpot README](https://github.com/librespot-org/librespot).

Kot začetna kandidatura je pregledan `librespot 0.8.0`, ne plavajoča veja `dev`.
Končno različico, feature flags, Rust/MSRV, Windows backend, licence in Cargo
odvisnosti je treba zakleniti v fazi P0. [Crate dokumentacija](https://docs.rs/librespot/0.8.0/librespot/).

Koda omogoča nalaganje konteksta z izbiro začetne skladbe, shuffle, repeat
konteksta, repeat skladbe, seek, premor in Connect transfer. `Spirc::load` po
dokumentaciji ne prepiše ročne queue. Javne Spirc kontrole predvsem upravljajo
lokalno napravo, ko je aktivna; same po sebi niso dokaz oddaljenega upravljanja
poljubne druge naprave. [Spirc](https://github.com/librespot-org/librespot/blob/v0.8.0/connect/src/spirc.rs),
[LoadRequest](https://github.com/librespot-org/librespot/blob/v0.8.0/connect/src/model.rs).

`SpClient` ima branje playlistov in rootlist, metapodatke, profile/followers,
lyrics, radio, autoplay kontekst in strani konteksta. Liked Songs ima dokumentiran
poseben context URI. To ni dokaz že implementiranega UI iskanja, knjižničnih
mutacij, Daily Mixes odkrivanja ali dislike operacij.
[SpClient](https://github.com/librespot-org/librespot/blob/v0.8.0/core/src/spclient.rs).

OAuth modul podpira browser authorization code s PKCE, refresh in token za
LibreSpot sejo oziroma Web API, če ima ustrezne scopes. Potrebuje ustrezen
client/redirect par. »Uporabnik ne vnese Client ID-ja« ne pomeni, da tehnično
Client ID ne obstaja ali da OAuth odpravi Spotifyjeve omejitve distribucije.
[LibreSpot OAuth](https://github.com/librespot-org/librespot/blob/v0.8.0/oauth/src/lib.rs).

Uradni Web API development način ima uporabniške/allowlist omejitve. Spremembe
leta 2026 med drugim omejujejo vsebino tujih playlistov, spreminjajo library in
playlist poti ter omejujejo rezultate iskanja. Običajne prijave ne smemo razglasiti
za rešitev teh omejitev brez dokazov. [Quota modes](https://developer.spotify.com/documentation/web-api/concepts/quota-modes),
[2026 migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide).

### 2.2 Obvezni P0 dokazi pred večjo izvedbo

Za vsak dokaz zabeležiti različico, datum, vrsto računa/market brez osebnih
podatkov, uporabljeni adapter, zahtevane scopes in rezultat. Uspeh na enem
lastnem developer računu ni dokaz podpore običajnim uporabnikom.

1. **Prijava in distribucija:** browser login, PKCE/state, ponovno odpiranje,
   refresh in preklic. Preveriti običajen Premium račun brez lastne developer
   konfiguracije in brez ročnega dodajanja na Apricot allowlist. Raziskati
   upstreamov podprt login/discovery flow; ne kopirati skrivnosti druge aplikacije.
2. **Knjižnica in mutacije:** Liked Songs read/save/remove ter ustvarjanje,
   dodajanje in odstranjevanje v namenskem testnem playlistu. Uporabniške
   podatke spreminjati šele v izrecno izbranem testnem računu/zbirki.
3. **Daily Mixes in personalizacija:** odkriti dejanske zbirke prijavljenega
   računa, prebrati njihovo vsebino in začeti iz izbrane skladbe. Testirati tudi
   Spotify-owned playlist, ne samo lastnega. Ne uporabiti globalno hardcodanih
   ID-jev ali iskanja po besedah »Daily Mix« kot nadomestila za osebne mixe.
4. **Queue in Connect:** prenesti isti kontekst med telefonom in Apricotom;
   potrditi dodajanje, odstranitev, premik, clearing, duplicate occurrences in
   remote updates. Javna API prisotnost za branje/dodajanje še ne dokazuje
   možnosti odstranjevanja ali preurejanja Connect queue.
5. **Zvok:** custom sink proti obstoječemu DSP, seek flush, pause, speed/pitch,
   pravilna ura in gapless. Izbrati arhitekturo na podlagi meritve, ne samo
   dejstva, da sink lahko sprejme PCM.
6. **Dislike:** ugotoviti natančno operacijo, kontekst, dovoljenja, možnost
   razveljavitve in dokaz učinka v Spotifyju. Like removal ni sprejemljiv dokaz.

Če je katera od dogovorjenih obveznih funkcij nedosegljiva, je rezultat P0
blokada z dokazom in predlogom rešitve. Funkcije ne sme tiho odstraniti iz plana
ali razglasiti »unsupported« samo zato, ker trenutni adapter nima metode.
Sprememba dogovorjenega obsega potrebuje uporabnikovo odločitev.

### 2.3 Kako dosežemo »vse, kar podpira knjižnica«

P0 izdela inventar zaklenjenih `core`, `metadata`, `playback`, `connect`,
`discovery`, `oauth`, `cache` in pripadajočih relevantnih protokolskih ukazov.
Vsako uporabniško zmožnost preslika v vrstico spodnjega manifesta. Helperji za
transport, kriptografijo in wire encoding ne potrebujejo svojega menija, morajo
pa podpirati zanesljivo uporabniško funkcijo. Nove odkrite zmožnosti dodati,
namesto da bi trenutni seznam razglasili za dokončno izčrpen brez inventarja.

Za vsako vrstico voditi: adapter/metoda/endpoint, različica, account/market
pogoji, pravice, UI, action ID, storage, automated test, live test in NVDA dokaz.
Razlikovati `source_verified`, `live_verified`, `implemented`, `accepted`,
`account_unavailable`, `service_unsupported` in `blocked`. Samo zadnji dve
ne pomenita uspešno zaključene obvezne funkcije.

## 3. Funkcionalni manifest

Oznake poti: **L** = v pregledani LibreSpot kodi obstaja relevantna podpora;
**A** = potreben dodatni adapter/API in preverjanje normalne prijave;
**V** = izvedljivost ali obseg nista potrjena. Oznaka ni acceptance kljukica.
Vse uporabniško relevantne in preverjeno podprte operacije so del prve 2.0.

### 3.1 Računi, Connect in predvajanje

| ID | Funkcija in acceptance namen | Pot |
| --- | --- | --- |
| S01 | Prijava, preklic, refresh, ponovna prijava in odjava brez developer nastavitev. | L/A/V |
| S02 | Več računov, izbira aktivnega, odstranitev shranjenega računa in izolacija podatkov. | Apricot + L |
| S03 | Premium/pravice/market/explicit restrictions, uporabne napake in ponovna pridobitev stanja. | L/A |
| S04 | Track playback, start paused, pause/resume, stop, exact seek in intervalni seek. | L |
| S05 | Next/previous, ponovni začetek po EOF in razlikovanje manual skip od naravnega konca. | L |
| S06 | Album/playlist/Liked Songs/radio kot celoten kontekst, ne samo ena razrešena skladba. | L |
| S07 | Enter na točno izbrani occurrence, tudi če ista skladba nastopa večkrat. | L + Apricot |
| S08 | Shuffle on/off, repeat off/context/track, spremembe prek telefona in ohranitev položaja. | L |
| S09 | Ročna queue pred kontekstom; add/play next/remove/reorder/clear in stanje po ponovnem zagonu. | L/A/V |
| S10 | Connect receiver, ime naprave, discovery, prevzem, transfer to/from Apricot in disconnect. | L |
| S11 | Seznam naprav, izbrana naprava in smiselne oddaljene kontrole glede na njene pravice. | A/V |
| S12 | Usklajevanje aktivnega medija, queue, položaja, glasnosti, repeat in shuffle brez zank. | L/A/V |
| S13 | Autoplay s Spotify kontekstom/priporočili, radio iz skladbe in vse dodatne podprte seed vrste. | L/A |
| S14 | Preload naslednje skladbe in gapless; brez podvajanja next ukazov. | L |
| S15 | Kakovost, normalizacija in pripadajoče napredne podprte zvočne nastavitve. | L |
| S16 | Apricot EQ, bass/volume boost, clipping protection, speed/pitch modes, reset in izhodna naprava. | DSP/V |
| S17 | Medijske informacije, dejanski kodek/kakovost, naslov izvajalca, album in current-item status. | L + Apricot |
| S18 | Obnovitev seje in položaja, bookmarks ter govor o času; glasba/govorna vsebina po D14. | Apricot + L/A |

Osnovna konfiguracija vsebuje bitrate 96/160/320, gapless, normalizacijo,
audio format/dither ter lokalne mape. Custom `Sink` je razširitvena točka.
To ne dokazuje lossless/FLAC ali delovanja DSP mostu.
[PlayerConfig](https://github.com/librespot-org/librespot/blob/v0.8.0/playback/src/config.rs),
[Sink](https://github.com/librespot-org/librespot/blob/v0.8.0/playback/src/audio_backend/mod.rs).

### 3.2 Iskanje, knjižnica in odkrivanje

| ID | Funkcija in acceptance namen | Pot |
| --- | --- | --- |
| S19 | Iskanje skladb, albumov, izvajalcev, playlistov, oddaj, epizod ter drugih dejansko podprtih vrst. | A/V |
| S20 | All/type filter, market-aware rezultati, strani, lokalni filter in stabilna izbira. | A + Apricot |
| S21 | Odpri album, diskografijo, artist detail/top tracks, podprte related artists in njihove zbirke. | L/A/V |
| S22 | Dejanski Spotify Like/Unlike skladbe: add/remove v Liked Songs aktivnega računa, preverjen učinek v uradnem klientu; Liked Songs pregled, play/from selection ter status v vrstici/playerju. | L read + A mutations |
| S23 | Shranjeni albumi, sledeni izvajalci, playlisti in podprta govorna knjižnica. | L/A |
| S24 | Follow/unfollow/save/remove/check status vseh podprtih entity vrst; bulk operacije, če so na voljo. | A |
| S25 | Recently played, podprti top tracks/artists in časovni filtri. | A/V |
| S26 | Home/Made for you: osebni izbori prijavljenega računa. | A/V |
| S27 | Daily Mixes: odkritje vseh razpoložljivih mixov, open, selected start, shuffle/repeat in refresh. | A/V, obvezno |
| S28 | Discover Weekly, Release Radar, daylist in druge dejansko vrnjene osebne zbirke. | A/V |
| S29 | Browse, genres/moods/categories, podprti charts in new releases. | A/V |
| S30 | Radio/stations: odkritje, seznam, izbrana skladba, nadaljevanje in podprti seed konteksti. | L/A |
| S31 | Profili, followers/following in podprti javni playlisti; navigacija brez izgube povratnega konteksta. | L/A |
| S32 | Dejanski dislike/hide/negative feedback in undo samo v ustreznem Spotify kontekstu. | A/V |

Vsako osebno zbirko identificirati z njenim URI/identiteto, ne prevedenim
naslovom. Število mixov se ne sme predpostaviti. Če novi račun še nima mixov,
je to stanje računa; če adapter ne zna odkriti obstoječih mixov, je to napaka
implementacije. Web API omejitve morajo biti vključene v izbiro adapterja.
[Spotify API pregled](https://developer.spotify.com/documentation/web-api),
[development migracija](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide).

### 3.3 Playlisti in spremembe uporabniških podatkov

| ID | Funkcija in acceptance namen | Pot |
| --- | --- | --- |
| S33 | Lastni, sledeni, collaborative in Spotify-owned playlisti; podprte mape/rootlist hierarhija. | L/A/V |
| S34 | Create: ime, opis, public/private in podprte collaboration pravice. | A |
| S35 | Rename/edit description/visibility ter collaboration/invite operacije, če so dejansko podprte. | A/V |
| S36 | Add enega ali več itemov, izbira cilja in položaja; duplicate occurrences so dovoljene. | A |
| S37 | Remove točno izbrane occurrence ali izbrane množice brez odstranitve vseh enakih skladb. | A |
| S38 | Move/reorder/replace podprtih itemov; concurrent edit zazna verzijo/snapshot. | A |
| S39 | Follow/unfollow/save playlist; razlikovati od urejanja vsebine in od brisanja lastništva. | A |
| S40 | Podprta playlist cover slika, prikaz podatkov, copy/share link in odpiranje v uradnem klientu. | L/A |
| S41 | Library/playlist sort/filter in podprto upravljanje map; UI sort ne preuredi Spotify playlista. | Apricot + A/V |
| S42 | Playlist spremembe s telefona, permission changes, izbris, stale snapshots in ponovni refresh. | A + Apricot |

Za Web API uporabiti aktualne `/items` operacije, kjer veljajo, in dejansko
dovoljene scopes. Pri reorder uporabljati `snapshot_id` oziroma ustrezen
verzijski mehanizem izbranega adapterja. Ne uporabljati slepega replace za
»odstrani eno skladbo« ali preurejanje brez zaščite drugih sprememb.
[Playlist reorder/replace](https://developer.spotify.com/documentation/web-api/reference/reorder-or-replace-playlists-items),
[playlist add](https://developer.spotify.com/documentation/web-api/reference/add-items-to-playlist).

### 3.4 Dodatna vsebina, metapodatki in integracija

| ID | Funkcija in acceptance namen | Pot |
| --- | --- | --- |
| S43 | Shows/podcasts in episodes: search, open show, play/resume, save/follow in status. | L/A |
| S44 | Podprti audiobook/chapter tipi: metadata, library, upravičenost, poglavja in resume. | A/V |
| S45 | Lyrics: sinhronizirana in nesinhronizirana, scroll/read, jump/seek kjer je timestamp. | L + Apricot |
| S46 | Transcript/podcast chapters, če jih Spotify za item dejansko vrača. | A/V |
| S47 | Cover art, artist/album details, credits/copyright/explicit/availability, kjer so vrnjeni. | L/A |
| S48 | Preview in drugi podprti preview tipi; ne nadomestiti full playback brez obvestila. | L/A |
| S49 | Lokalni Spotify playlist itemi: prikaz, prepoznava in lokalno predvajanje samo ob dokazani podpori/poti. | L/V |
| S50 | `spotify:` URI, `open.spotify.com` URL in dovoljena short-link razrešitev iz Direct link/clipboard. | L + Apricot |
| S51 | Favorites, zgodovina, bookmarks, last session in mešani Apricot playlisti s trajnimi referencami. | Apricot |
| S52 | Background player, tray/media keys, status, podatki in običajni player hotkeys glede na capability. | Apricot + L |
| S53 | Metadata/audio cache, limit/clear, reconnect/offline stanja in zaščita credentials pri čiščenju. | L + Apricot |
| S54 | Diagnostika capability/account/device stanja brez tokenov, osebnih URL-jev ali skrivnosti. | Apricot |
| S55 | Celotna tipkovnična/NVDA pot, kontekstni meniji, Action Finder in lokalizacija. | Apricot |

Metapodatkovni tip ni dokaz predvajanja: `video` v protokolu ne pomeni delujočih
Spotify videov, audiobook rezultat ne pomeni pravice do vseh poglavij, podatek
o local-file mapi ne dokazuje Connect prenosa lokalnih datotek. Te razlike
preveriti posebej. [Metadata moduli](https://github.com/librespot-org/librespot/blob/v0.8.0/metadata/src/lib.rs),
[Connect local restrictions](https://github.com/librespot-org/librespot/blob/v0.8.0/connect/src/state.rs).

### 3.5 Obvezna evidenca meja

P0 izrecno preveri offline listening/licenciranje cachea, lossless, crossfade,
Spotify video/Canvas, Smart Shuffle, DJ, Jam, private session, lokalne datoteke,
Wrapped ter vse druge odkrite uporabniške zmožnosti. Kar ni podprto v zaklenjeni
knjižnici/dostopnem vmesniku, dobi dokumentiran razlog. Tega ne prikazati kot
delujočo funkcijo na podlagi imena protokolskega polja.

Spotify audio cache ni uporabniški download, FFmpeg export ali kopija za drug
predvajalnik. Obstoječe Download/Copy stream URL/Save edit copy/Replace original
akcije za Spotify ne smejo avtomatsko postati izvozne poti. Če obstaja dejanska
dovoljena in podprta operacija, jo posebej vpisati in dokazati; sicer prikazati
smiselno nedostopnost dejanja. Markers/preview lahko imajo lokalno playback
uporabo, če ne izvažajo vsebine in so tehnično podprti. To ne zmanjša zahtev za
EQ/speed/pitch ali bookmarke.

## 4. Spotify zasloni in navigacija

### 4.1 Glavni meni in Spotify vstop

Dodati `spotify` v Rust `CUSTOMIZABLE_MAIN_MENU` in pripadajoči koncept
`MAIN_MENU_CUSTOMIZABLE_ITEMS`. Predlog mesta: ob drugih spletnih integracijah,
za AudioVault. Skrivanje ne onemogoči `open_spotify` bližnjice. Settings,
Update Available in Exit ostanejo stalni. Spotify vstop je dosegljiv tudi brez
prijave; takrat omogoča login in pojasnilo, ne slepega izginotja.

Predlagani korenski seznam po Enter na Spotify:

1. Search
2. My library
3. Liked Songs
4. Playlists
5. Made for you / Daily Mixes
6. Home / Discover
7. Recently played
8. Radio
9. Podcasts and shows
10. Audiobooks, če adapter podpira ta tip
11. Spotify queue (število ročno dodanih itemov)
12. Connect devices
13. Accounts / prijava / odjava
14. Spotify settings
15. Back

V meniju ne izpisovati vsake album/playlist operacije: te so v ustreznem
podmeniju in kontekstnem meniju. Tako so vse funkcije dosegljive, glavni seznam
pa ostane uporaben. Odsotnost vsebine računa ločiti od nepodprte zmožnosti.
Pri pomembni nepodprti funkciji ponuditi razlago; ne pustiti inertnega gumba.

### 4.2 Podseznami

- **My library:** Playlists/folders, Liked Songs, Albums, Artists, Shows,
  Episodes in drugi dokazano podprti shranjeni tipi; lokalni filter in sort.
- **Made for you:** Daily Mixes, Discover Weekly, Release Radar, daylist in
  druge dejansko vrnjene osebne zbirke. Daily Mixes ima neposredno bližnjico.
- **Search:** query edit, type filter (All in podprti tipi), Search, results,
  Next page/Load more, Back. All rezultati imajo izgovorjen tip.
- **Playlist/album:** naslov in owner/artist, opis/status, list itemov,
  Play, Shuffle play, Filter/sort view in contextual edit glede na pravice.
- **Artist:** vrnjene podprte top tracks, albums/releases, saved/follow status,
  radio/related in podatki. Manjkajoč API podatek ni prazna zbirka.
- **Show/book:** list epizod/poglavij, duration/progress, resume in pravice.
- **Queue:** jasno ločeni »Ročno dodano« in »Naslednje iz konteksta«; bodoči
  items iz playlist konteksta niso ročna queue in jih Clear queue ne izbriše.
- **Devices:** aktivna naprava, lokalna/oddaljena, dostupnost kontrol, transfer
  in Back. OS zvočna naprava je drug pojem od Spotify Connect naprave.

### 4.3 Fokus in identiteta

Razširiti `Route`, `ScreenKind` in route parameters za Spotify hub/search/
library/collection/detail/mixes/queue/devices/accounts/settings. Uporabiti
obstoječi `NavigationStack`, ne vzporedne Win32 navigacijske zgodovine.

Vsak frame shrani account, URI zbirke, query/filter/sort, page cursor, selected
occurrence ID, column in focus ID. Escape gre eno raven nazaj. Povratek iz
playerja obnovi točno vrstico in kontrolo, tudi po remote refreshu. Ne uporabljati
samega indeksa ali samega track URI-ja za duplicate occurrences.

Network completion ima account epoch + request generation + route/query ID.
Zastarel odgovor ne sme prepisati novega iskanja, drugega računa ali zagnati
starega medija. Background refresh ne odpira zaslona in ne jemlje fokusa.

## 5. Kontekstni meniji in bližnjice

### 5.1 Meniji

**Like je obvezna funkcija.** Na skladbi prikazati »Všeč mi je« oziroma
»Odstrani iz Liked Songs« glede na potrjeno stanje aktivnega Spotify računa.
Dejanje je dosegljivo v seznamu in playerju ter prek `spotify_toggle_saved`
bližnjice. Uporabiti dejansko Spotify knjižnično mutacijo; lokalna oznaka,
Apricot Favorite ali druga zbirka niso nadomestilo. Uspeh potrditi s Spotify
stanjem, napako jasno oznaniti in ne pustiti lažne oznake liked. Sprememba
Like/Unlike iz uradnega klienta se ob ustreznem refreshu odrazi tudi v Apricotu.

Track/episode menu: Play, Add to Spotify queue, Play next kjer je podprto,
Save/Remove from Spotify library, dejanski Dislike/Undo kjer velja, Add to
Spotify playlist, Apricot Favorite, Add to Apricot playlist, artist/album/show,
Radio, Details, Copy link, Open in Spotify. Izbrana vrstica je cilj v listu;
trenutni medij je cilj v playerju. Posodabljanje izbire med network requestom
ne sme spremeniti tarče mutacije.

Playlist menu: Open, Play, Shuffle play, Save/Follow oziroma Remove/Unfollow,
Create, Edit details, dovoljena Add/Remove/Reorder, Cover, Share/Copy link in
Details. Read-only playlist ne dobi aktivnih write kontrol. »Odstrani iz moje
knjižnice« ni lažno »Izbriši playlist«. Delete je vezan na vrsto in kontekst
izbrane postavke, nikoli generično destructiven.

Queue menu: Play selected, remove/move/clear samo za potrjeno podprte operacije
in točno occurrence; remote conflict ponudi refresh. Applications in Shift+F10
morata oba odpreti isti meni. Vsako dejanje obstaja tudi brez miške.

### 5.2 Potrjeni predlog novih privzetih bližnjic

Spodnje kombinacije ob pripravi niso v obstoječem 91-action katalogu; pred
merge jih ponovno preveriti proti novi kodi, uporabniškim overrides in NVDA
kombinacijam. Predlog je potrjen kot izvedbena osnova; tipke še niso
implementirane. Vsa dejanja morajo biti
v kanoničnem action registru, Settings/Shortcuts, lokalizaciji in Action Finderju.

| Action ID | Predlog tipke | Scope |
| --- | --- | --- |
| `open_spotify` | Ctrl+Alt+C | Global |
| `spotify_search` | Ctrl+Alt+Shift+Y | Global |
| `spotify_library` | Ctrl+Alt+Shift+L | Global |
| `spotify_liked_songs` | Ctrl+Alt+Shift+F | Global |
| `spotify_playlists` | Ctrl+Alt+Shift+P | Global |
| `spotify_daily_mixes` | Ctrl+Alt+Shift+M | Global |
| `spotify_queue` | Ctrl+Alt+Shift+Q | Global |
| `spotify_devices` | Ctrl+Alt+Shift+O | Global |
| `spotify_accounts` | Ctrl+Alt+Shift+C | Global |
| `spotify_toggle_saved` | Ctrl+Shift+I | Spotify list/player |
| `spotify_dislike` | Ctrl+Shift+H | Spotify list/player, samo podprt kontekst |
| `spotify_radio` | Ctrl+Alt+Shift+R | Spotify list/player |

Rare operacije (cover, visibility, invite, folder management, undo dislike,
autoplay settings) imajo svoja configurable action IDs in kontekstne menije.
Obstoječi katalog zahteva ne-prazne privzete bližnjice; pri dodajanju se odloči
in testira coherent katalog vseh novih dejanj. Ne dodati skritih handlerjev brez
registry vnosa. Ne trditi, da ostaja celoten katalog omejen na 91 dejanj;
baseline 91 mora ostati pokrit, novi katalog pa se razširi.

### 5.3 Ponovna uporaba obstoječih tipk

| Obstoječa tipka | Spotify vedenje |
| --- | --- |
| Enter / Space / Escape | Odpri/začni izbrano; play/pause samo v playerju; ena raven nazaj. |
| Ctrl+PageUp / Ctrl+PageDown | Previous/Next v aktivnem Spotify kontekstu. |
| Shift+S / R | Spotify shuffle; repeat off/context/track s kratkim oznanilom. |
| Left/Right in obstoječi modifikatorji | Seek prek Spotify, ne v zastarelem PCM medpomnilniku. |
| Up/Down, V, T, F7 | Glasnost, status glasnosti/časa in podrobnosti po obstoječem vzorcu. |
| F2 / F3 / F4 / O | Lokalni boost, bass, EQ, OS output device; remote target nima teh lokalnih kontrol. |
| S/D, Ctrl+Up/Down, Ctrl+0 | Lokalna speed/pitch obdelava in reset. |
| Ctrl+Shift+Y | Spotify lyrics skozi obstoječi lyrics UI. |
| Ctrl+Shift+T/C | Transcript/chapters, samo kadar so dejansko na voljo. |
| Ctrl+Shift+Q | Add to Spotify queue za Spotify cilj. Drugi viri uporabljajo obstoječo Apricot queue. |
| Ctrl+F / Ctrl+Shift+F | Apricot Favorite add/remove; ne Spotify like. |
| Ctrl+P | Obstoječi Apricot playlist; za Spotify playlist dodati ločeno poimenovano dejanje. |
| Ctrl+Shift+N | Create Spotify playlist v Spotify playlist kontekstu; drugje obstoječe vedenje. |
| Ctrl+L / L | Trajni Spotify link, ne token ali CDN URL. |

Globalna Ctrl+Alt+Q ostane obstoječa Apricot queue; ločena Spotify queue ima svoj
vstop. V text edit polju tiskljive player tipke ne izvajajo player dejanj.
Settings shortcut capture ima prednost pred globalnimi akcijami. Konflikte
preverjati na kanoničnih chordih in dejanskih prekrivajočih scope/capability,
ne na dobesednih stringih.

## 6. Pogodba predvajanja in Connect

### 6.1 Izbrana skladba in kontekst

Za Spotify playlist/album shraniti `context_uri`, snapshot/revision in occurrence
UID ali ustrezen stabilni identifikator. `LoadRequestOptions.playing_track`
izbere točno pojavitev. Pri shuffle najprej igra izbrana skladba, nato veljavni
shuffle kontekst; prikazan sort/filter ne spremeni izvornega zaporedja.
Velik playlist se nalaga po straneh; konec prve strani ni konec playlista.

Ročna queue se ohrani ob menjavi konteksta po D16. Repeat track ponavlja
trenutno skladbo do izklopa ali manual Next; repeat context ponavlja kontekst.
Autoplay se uporabi šele ob prazni ročni queue, končanem kontekstu in izključenem
repeat. Če upstream drugače obravnava kombinacijo queue/repeat, P0 to pokaže
in adapter izvede potrjeno pogodbo brez dveh neodvisnih next state machineov.

Transport, aktivna naprava in potrjena queue imajo enega avtoritativnega
lastnika: Spotify/Connect runtime. Apricot projicira stanje in pošilja ukaze;
ne ugiba queue porabe iz »ukaz poslan« ali »medij razrešen«. Ločiti začet
pending command od potrjenega rezultata in remote update.

P0 potrdi queue snapshot/event integracijo: stock Spirc nima vseh teh mutacij
kot javnih metod. Po potrebi ozka upstream razširitev ali dodatni adapter z
testi; ne brati zasebnih Rust polj z unsafe ali voditi lokalne lažne queue.
[Spirc javni API](https://github.com/librespot-org/librespot/blob/v0.8.0/connect/src/spirc.rs),
[Web API queue](https://developer.spotify.com/documentation/web-api/reference/get-queue).

### 6.2 Prevzem in oddaljena naprava

Incoming Connect transfer: shraniti položaj prejšnjega vira, ustaviti njegov
zvok, zamenjati active playback owner in potrditi Spotify start. Brez dveh
hkratnih audio izhodov. Ohraniti prejšnji Apricot return frame za namerno
vrnitev. Poročati en kratek dogodek, ne ukrasti fokusa.

Transfer iz Apricota na telefon/drugo napravo ustavi lokalni sink. Apricot
prikazuje remote stanje in uporablja samo podprte remote ukaze; lokalni
speed/pitch/EQ/boost niso zmožnosti tuje naprave. Ne klicati neaktivnega Spirc
in uspeha domnevati iz `Ok`. Povratek na Apricot obnovi potrjeni kontekst in
lokalno DSP stanje računa. Ime naprave mora biti prepoznavno, tudi ob več
Apricot instalacijah.

Pri preklopu računa ustaviti/odjaviti njegovo lokalno Spotify sejo in sink ter
preklicati njegove requests. Ne ustaviti brez razloga predvajanja starega
računa na drugi fizični napravi. Nova seja začne z novim epochom, brez zvočnih
paketov ali podatkov starega računa. Naenkrat je lokalno aktiven en račun.

### 6.3 Nedostopnost in resume

Posamezen unplayable item dobi reason, ostane v listu in se pri nadaljevanju
preskoči. Braniti se pred neskončnim kroženjem: vsako occurrence v enem
poskusu največ enkrat; če so vse nedostopne, ustaviti. Več zaporednih preskokov
združiti v uporabno obvestilo. Explicit policy/market/entitlement razlogov ne
obravnavati kot network retry. Revoked login ali izguba povezave ustavi in
ponudi ustrezno recovery; queue se ne izbriše.

Music Enter začne pri 0. Govorna vsebina prebere preverjeni Spotify progress,
kjer je na voljo, sicer Apricotov account-scoped položaj. Kadar ni informacij
o svežini dveh virov progressa, jih ne primerjati na izmišljenem timestampu.
P0 določi prednost in conflict strategijo. Resume last session/explicit bookmark
ima jasno namero in obnovi svoj položaj. Položaj shraniti tudi ob Exit,
account switch, takeover in normalnem shutdownu.

## 7. Arhitektura v obstoječem Rust projektu

### 7.1 Meje modulov

Predlog novega `rust/crates/apricot-spotify` z zaklenjenimi LibreSpot
odvisnostmi. Native UI ne pozna tokenov, protobuf objektov ali raw endpointov.
Fizično število modulov prilagoditi kodi; odgovornosti ostanejo ločene:

- session/auth/accounts in account capability;
- browse/search/library/personalized adapterji;
- playlist mutation in revision reconciliation;
- Connect transport/queue/device state;
- audio sink bridge in telemetry brez osebnih podatkov.

`apricot-core`: `MediaSource::Spotify`, tipizirana trajna Spotify identiteta,
entity/occurrence/context/device/repeat tipi in capability omejitve dejanj.
`apricot-app`: Spotify controllers in screen/menu modeli ter povezava v
`Application`, `PlayerSession` in obstoječi Action Finder.
`apricot-playback`: backend routing in adapter za shared commands/events,
uporaba obstoječe audio-chain logike brez podvajanja mpv preset pravil.
`apricot-storage`: zaščitene seje, account nastavitve in reference/migracije.
`apricot-ui-windows`: native Win32 prikaz in kontrola modelov, ne dodatni
monolitni spletni klient v `win32.rs`.

Pred širjenjem obstoječega PlaybackEngine preveriti dejanske callers. Če Spotify
potrebuje context/queue capability, dodati ozko tipizirano plast/coordinator;
ne siliti vseh source resolverjev v iste Spotify posebne parametre.

### 7.2 Trajni podatki in capabilities

Trajno shraniti Spotify URI/ID, entity kind in uporabne title/artist fallbacke.
Ephemeral token, CDN URL, decipher/audio key ali OAuth code nikoli ne sodijo
v MediaItem, favorites, clipboard, običajen settings JSON ali export podatkov.
Availability je lastnost aktivnega računa/naprave/časa, ne nespremenljiv del
track reference. Apricot globalne zbirke se ne podvajajo po računu; Spotify
zasebna knjižnica, progress in queue cache pa se izolirajo.

Capability ima vsaj Read/Play/Save/Unsave/EditPlaylist/ManageQueue/Dislike/
Lyrics/Transcript/Chapters/LocalDsp/RemoteControl in reason, če ni na voljo.
Unknown se ne preslika v Yes. Meniji, hotkeys in Action Finder uporabljajo isti
capability model, da disabled menu ne pušča aktivne destruktivne bližnjice.

### 7.3 Async in state machine

Vsak request/command/event nosi account epoch, playback generation in relevantni
request/occurrence ID. Cancel/back/account switch/new selection prekliče stare
zahteve; late completion nima pravice spreminjati nove seje. Await/Drop/worker
join ne blokira UI thread-a. Deadline, bounded retries in cancellable shutdown.

Critical events se ne zavržejo ob polnem position kanalu ali modalnem dialogu.
Position se lahko coalesce, start/pause/failure/ended/account/device/queue
changes pa ohranijo vrstni red ali obnovljivo avtoritativno stanje.
Snapshot refresh po reconnectu prepreči nadaljevanje s fantomsko queue.

Mutacije so pending do potrditve. Idempotent save/unsave sme retry po pravilih;
create/add/reorder ne sme slepo ponoviti po izgubljenem odgovoru in ustvariti
dvojnikov. Najprej reconciliation s server stanjem/revision. 401 -> omejen
refresh; 403 -> permissions/capability razlaga; 429 -> Retry-After/backoff;
network/service napaka -> ohranitev podatkov in uporabna recovery.

## 8. Zvok: obvezna izvedljivost, ne predpostavljena povezava

LibreSpot naj ostane lastnik Spotify download/decode/seek in Connect transporta.
Apricot naj uporablja isti lokalni DSP opis kot za mpv. V P0 primerjati:

1. Custom LibreSpot sink -> omejen PCM medpomnilnik/stream -> obstoječi libmpv
   filtri in output. To je prva kandidatura zaradi reuse, ne že izbrana rešitev.
2. Custom sink -> shared DSP adapter z istimi EQ/speed/pitch algoritmi in
   preverjeno output implementacijo, če mpv stream most ne prestane seek,
   latency, gapless ali clock preverjanja.

Navadna dolga pipe v mpv ni zadosten dokaz: lokalni mpv seek ne sme premikati
samo buffered PCM, speed ne sme razhajati source clocka in Connect prikaza.
Spotify seek mora prekiniti staro generation, izprazniti buffer in stare
decoder/output pakete, sprožiti dejanski source seek in obnoviti potrjeno uro.
Pri speed/pitch testirati backpressure, preloading, buffered duration, EOF,
pause ter čas lyrics/bookmarkov. Prikaz časa je vsebinski položaj, ne število
sekund izhodnega sinka brez preslikave.

No disk PCM export/temp recording na normalni poti. No dvojni volume/mixer ali
dvojna normalizacija: določiti, katera plast izvaja gain. Spotify normalization
in Apricot ReplayGain izbrati/uskladiti; boost in EQ morata ohraniti clipping
protection. Normalizacija ne sme samodejno spremeniti shranjenih nastavitev
drugih virov. Default bitrate predlog 320, gapless on; končne defaults zabeležiti
po meritvah in pregledu.

BPM/signal analysis, markers in clip preview preveriti kot lokalne playback
zmožnosti brez izvoza. Format status oznani dejansko Spotify/source kakovost in
loči to od PCM/DSP izhoda. Lossless ne prikazati samo zato, ker je sink float.

## 9. Accounts, settings, cache in mešane Apricot zbirke

### 9.1 Shranjevanje in prijava

Uporabiti preverjeni upstream OAuth oziroma podprto discovery prijavo v sistemskem
browserju. Dialog ima Open browser, Waiting, Cancel, Retry in razumljiv status.
PKCE/state/redirect preverjanje, omejen loopback listener in deadline; brez
gesla v Apricot dialogu, kopiranja cookies ali ročnega token paste kot normalne
poti. Scopes dokumentirati glede na dejanske feature adapterje.

Credential storage skozi OS zaščito (Windows DPAPI ali ustrezna obstoječa
platform abstrakcija; prihodnji macOS Keychain). Tokeni ne gredo v diagnostiko,
data export ali generični backup. Če portable profil preide na drug računalnik,
ponovna prijava je razumljivo stanje. Clear audio/metadata cache ne izbriše
login blob-a; Logout/Remove account pa ga odstranita po ustrezni nameri.

### 9.2 Spotify settings

Po računu: shuffle, repeat mode, autoplay, kakovost, normalization in podprti
napredni parametri, local DSP/session defaults, zadnji kontekst in resume.
Po instalaciji: device name/type in discovery, cache folder/limit, refresh
policy ter privzeti account izbor. Expert settings izpostavijo vse relevantne
PlayerConfig/ConnectConfig možnosti s smiselnimi imeni, validacijo in opisi;
ne pa nevarnih wire/debug parametrov brez uporabniške vrednosti.

Autoplay predlog: najprej prebrati veljavno account preference, kjer je
preverljivo; sicer začetni default off in jasen toggle. Account/Connect sprememba
repeat/shuffle posodobi Spotify state, ne globalnih nastavitev drugih virov.
DSP nastavitve ohranijo obstoječo logiko presetov, vendar so Spotify specifične
sejne vrednosti izolirane. Settings reset ne briše knjižnice ali playlistov.

### 9.3 Apricot playlists, bookmarks in zgodovina

Apricot Favorite in Spotify like sta dve jasno poimenovani operaciji. Trajne
Spotify reference se odprejo z aktivnim računom; zaseben playlist drugega računa
ne sme biti samodejno pripisan ali mutiran pod napačnim računom.

Mešani Apricot playlist (local/YouTube/Spotify) ostane Apricot playback sequence,
ne postane Spotify playlist ali mešana Spotify queue. Ob Spotify postavki
coordinator ustvari ustrezen lokalni Spotify kontekst; ročna Spotify queue ima
potrjeno prednost. Po potrjenem koncu Spotify postavke in ročne queue se vrne
v mešano Apricot sequence. Native Spotify autoplay se v tem posebnem lokalnem
sequence načinu ne sme potegovati z Apricot Next. Ta začasna omejitev velja za
lokalni sequence kontekst in ne prepiše shranjene Spotify autoplay preference.
Incoming Connect izbira
drugega konteksta zamrzne to sequence; ne sme ob naslednjem EOF nenadoma vrniti
YouTuba. Nadaljevanje mešane sequence je namensko uporabniško dejanje.

Bookmarks hranijo vsebinski položaj. Current-item history se zapiše po potrjenem
začetku, ne po ukazu ali neuspešnem loginu; normalni item transition ne ustvari
dvojnikov iz lastnega in remote dogodka. Data import/export/migration ohrani
Spotify reference in obstoječe podatke, brez credential exporta.

## 10. Dostopnost in lokalizacija

Uporabiti obstoječe Win32 list/edit/button površine in screen modele. Vsaka
vrstica ima smiselno ime/tip/stanje: track title, artist, album, duration,
liked/unavailable; playlist title, owner, count kjer je znan, write/read-only.
Multi-select, sort/filter in page loading morajo biti izvedljivi s tipkovnico.

Enter/Escape/Tab preveriti na dejanski Win32 message loop in `IsDialogMessage`,
ne samo na synthetic controller testih. Applications in Shift+F10 sta enaki
poti. Item updates ne spremenijo selection/focus; remote play je eno status
obvestilo, ne avtomatsko odpiranje playerja. Like success/failure ni dvojno
speech/UIA/live-region obvestilo. Position ticks se ne oglašajo.

Lyrics/transcript imajo ustavljeno samodejno sledenje med branjem, možnost
ponovne sinhronizacije in jump/seek samo ob timestampu. Spoken unavailable
razlog ne sme izpisati 50 zaporednih obvestil. Account login browser, cancel in
return imajo preverjen focus path. Native control role/state/value/selection
preveriti z NVDA in braille, ne samo accessibility tree snapshotom.

Besedila v `locales_json`: labels, shortcuts, confirmations, network/rights
errors in status placeholders. Ohraniti English fallback in loader validacijo
vseh 27 jezikov; pred objavo zaključiti prevode po obstoječem workflowu.

Shared core ne ve za HWND/Win32. Pri prihodnjem macOS portu prenesti isti
Spotify manifest, accounts/Keychain, actions in VoiceOver acceptance; ne narediti
okrnjene macOS Spotify podpore. Windows implementacija ostaja prva po obstoječem
macOS planu, ne vzporedna nova implementacija.

## 11. Izvedbene faze in odvisnosti

| Faza | Predpogoj | Delo | Izhodni dokaz |
| --- | --- | --- | --- |
| P0 | Ta plan in ločena izolacija za raziskovanje | Zmogljivostni inventar, login/distribucija, library mutations, mixes/dislike, queue/Connect in audio spike. | Dokazna matrika brez skritih blokad; izbrana version/adapter/DSP strategija. |
| P1 | P0 login + crate odločitev | Source/entity/capability tipi, account storage/auth, epochs, hub/routes/action registry. | Login/cancel/relogin/switch; prvi native screen NVDA walkthrough. |
| P2 | P0 audio + P1 | Lokalni playback/DSP, EOF/seek/pause, session ownership in Connect takeover. | Real engine + lokalni/remote playback in audio/time dokazi. |
| P3 | P0 queue + P2 | Context selection, paging, queue synchronization, repeat/shuffle/autoplay/device UI. | Izbrana occurrence -> queue -> kontekst; telefon/Apricot usklajena. |
| P4 | P0 browse/mutation + P1 | Search, library, likes/dislike, artist/album/show, vse playlist operacije in permissions. | Testna knjižnica/mutacije, concurrent edit/reconciliation, NVDA. |
| P5 | P0 personalizacija + P3/P4 | Daily Mixes, Home/Made for you, radio, recently played, vsi dodatni podprti discovery tipi. | Dejanske osebne zbirke več računov, ne mock-only demos. |
| P6 | P2–P5 | Lyrics/govorna vsebina, favorites/history/bookmarks/mixed playlists, settings/cache/diagnostics. | End-to-end podatki, cancel/late responses, account isolation, lossless migration. |
| P7 | P1–P6 + zaključena Python–Rust pariteta | Celoten acceptance manifest, performance/soak, installer/portable, privacy/credential checks. | Release evidence in uporabnikova ločena odobritev objave. |

Posamezne knjižnične/data modele in UI se lahko razvija po preverjeni odvisnosti,
vendar nepodprte personalizacije ali audio clocka ne sme odkriti šele v P7.
Vsaka faza ima majhen lokalni diff in review; ne mešati nove Spotify izvedbe
s Claudovimi popravki prejšnjih 26 ugotovitev. Plan ni pooblastilo za commit,
push, tag, namestitev ali release.

## 12. Acceptance testi

### 12.1 Avtomatski

- Parser/URI/entity/availability tipi, unknown optional fields in oba relevantna
  response formata. Search paging in playlist occurrence duplicate identiteta.
- Save/unsave/playlist revision tests: stale snapshot, izgubljen odgovor,
  retry brez dvojnega add/create in permissions, ki se spremenijo med zahtevo.
- Context start iz sredine, shuffle on/off, repeat tri modes, preserved queue,
  queue next precedence, EOF ena transition in konec strani brez premature stop.
- Phone/Apricot concurrent commands, device transfer, reconnect snapshot in
  ack/remote echo deduplikacija. Callback starega računa/newer selection zavrnjen.
- Playback critical events ob modalnem dialogu in polnem position bufferju;
  start paused, retry generation in EOF restart, posebej glede na review napake.
- Music start at zero, voice resume, last-session resume, Exit/takeover/switch
  položaj, bookmarks ter vsebinski čas pri speed/pitch.
- Credential/cache purge meje, read-only/corrupt storage in failure rollback;
  stable Python profil se ne spreminja, migracije so atomarne in povratne.
- Action/menu/shortcut catalogs, scopes, canonical conflicts, edit capture,
  prevodi/placeholderji, hidden Spotify menu z aktivno globalno bližnjico.
- Contract fixtures za vsak dodatni/neuradni adapter, sanitized brez tokenov;
  schema drift napaka ni »prazna knjižnica«. Live testi so ločeni od offline CI.

Za Rust baseline uporabiti `cargo test --workspace --locked`,
`cargo clippy --workspace --all-targets --locked -- -D warnings` in
`git diff --check`; dodatni crate checks skladno s projektom. Python referenčne
preveritve ostanejo del rewrite parity gatea, kjer spremembe posegajo v primerjavo.

### 12.2 Obvezni realni scenariji

| Test | Pot | Pričakovani rezultat |
| --- | --- | --- |
| R01 | Sveža namestitev -> Spotify -> browser login -> return | Brez developer nastavitev, čist focus path in potrjeni capability. |
| R02 | Preklic login, napačen/potekel callback in ponovni poskus | Ne zamrzne UI; brez shranjene delne seje. |
| R03 | Dva računa z različnimi knjižnicami in mixes | Ni cross-account listov, queue, govornih obvestil ali zvoka. |
| R04 | Enter na tretji skladbi playlista | Začne tretjo; next četrta; sorting view ne spremeni izvorne vrste. |
| R05 | Ista skladba dvakrat v playlistu | Izbere pravo occurrence; remove/move zadeva samo izbrano. |
| R06 | Večstranski playlist, shuffle in repeat | Noben zgodnji konec strani, nobena izgubljena skladba ali dvojni next. |
| R07 | Ročna queue -> izbira skladbe drugega playlista | Izbrana takoj, stara ročna queue ohranjena, nato novi kontekst. |
| R08 | Phone add/remove/reorder in Apricot ukazi | UI sledi potrjeni Connect queue; konflikt ne ustvarja druge vrste. |
| R09 | YouTube/local playback -> phone transfer to Apricot | Prejšnji position shranjen, en audio owner, focus ostane. |
| R10 | Apricot -> remote device -> Apricot | Potrjeni context/position; remote DSP kontrole ne lažejo. |
| R11 | Playlist konec + autoplay/repeat kombinacije | Točno dogovorjena prednost, Spotify recommendations samo ob pravih pogojih. |
| R12 | Nedostopna skladba / vse nedostopne / izpad omrežja | Skip posamezne, končen stop; auth/network ne izbriše queue. |
| R13 | Like skladbe v Apricotu -> pregled v uradnem Spotify klientu -> Unlike; nato Like/Unlike v uradnem klientu -> refresh Apricota; ločeno dislike/undo | Like dejansko doda skladbo v Liked Songs pravega računa, Unlike jo odstrani; status se uskladi v obeh smereh. Favorites in dislike ostaneta ločena. |
| R14 | Create/edit/add/remove/reorder ter phone concurrent edit | Brez izgube tujih sprememb ali napačne occurrence; permission jasno. |
| R15 | Daily Mixes dveh običajnih računov | Pravi osebni mixi, vsebina, izbrana skladba in njihova osvežitev. |
| R16 | Lyrics, timestamp jump in branje z NVDA | Pravilna ura, brez focus steal ali neskončnega avtomatskega branja. |
| R17 | Govorna vsebina, bookmark, Exit in resume | Music/voice pravila; položaj shranjen tudi ob celotnem izhodu. |
| R18 | EQ/boost/output/speed/pitch + seek med bufferjem | Izmerjena audio parity, clipping policy, brez starega PCM ali clock drift. |
| R19 | Gapless in preload z dejanskim albumom | Prehod brez dodatne umetne vrzeli; skladba/status enkrat zamenjana. |
| R20 | Modalni dialog, background playback in remote spremembe | UI/engine state ne izgubi critical eventov. |
| R21 | Slow search A -> B; start A -> B; account A -> B | Late completion A ne prepiše ali zažene ničesar v B. |
| R22 | Favorites/history/mixed Apricot playlist | Trajne reference, correct owner, Connect takeover ne sproži poznega Apricot Next. |
| R23 | Keyboard-only vse screens in shortcut edit | Enter/Escape/Tab/Shift+F10 delujejo; text entry in capture sta varna. |
| R24 | Installer in portable iz istega builda, update/rollback | Spotify dependencies vključene; account/profile ohranjena brez credential izvoza. |
| R25 | Daljše predvajanje, reconnect, sleep/wake in izhod | Brez runaway retry, deadlocka, UI freeze, leakage ali poznega zvoka. |

Real accounts/phone/network/engine/NVDA dokazov ne nadomesti mock test ali
launch aplikacije. Live destructive library tests uporabljajo namensko testno
zbirko, ne uporabnikove običajne knjižnice. Test evidence ne hrani skrivnosti.

### 12.3 Performance in release vrata

P0 izmeri cold/warm login, hub/search, time to first audio, seek recovery,
transition/gapless, UI responsiveness ter CPU/RAM/cache in določi merljive
budžete glede na dejansko okolje. Network latenco ločiti od UI/adapter/DSP
overheada. Na normalni uspešni poti ne dodati dvojne ekstrakcije, podvojenega
metadata requesta ali čakanja za priporočila pred začetkom skladbe.

Prva javna 2.0 zahteva: zaključena Python–Rust pariteta in review popravki,
vsi obvezni Spotify manifest items z dokazi, realni account/Connect/DSP/NVDA
scenariji, nedestruktivna data/update/rollback pot ter preverjena installer in
portable artefakta istega commita. Skladno z aktivnim macOS pipelineom se
kasneje doda poln macOS Spotify manifest in ustrezen DMG acceptance.

Noben obvezni item se ne zapre z »API se je spremenil«, »mock je zelen« ali
»kontrola je narisana«. Dokazano nepodprta nova funkcija se dokumentira in
morebitna izjema od uporabnikove zahteve izrecno potrdi. Objavo uporabnik
odobri posebej po konkretnem končnem rezultatu.

## 13. Navodilo za Claudeovo nadaljevanje

Uporabnik je celoten plan, vključno z UI/action delom, potrdil 30. 9. 2026.
Najprej pregledati ta plan, nato začeti s P0, ne s širokim UI scaffoldom.
Za vsak spike vrniti dokaz,
omejitve in odločitev, posebej za login/distribucijo, Daily Mixes, dislike,
polno Connect queue in PCM/DSP clock. Skupna zasnova je potrjena; implementacijo
voditi po preverjenih odvisnostih v izoliranem lokalnem okolju. Spremembo
dogovorjenega produktnega obsega ali objavo uporabnik odobri posebej.

Med izvedbo vzdrževati `SPOTIFY_PARITY_MANIFEST.md` in kratko dokazno evidenco
po feature ID-jih. Obstoječi Rust parity gate ostane nedotaknjen. Vsi zgoraj
opisani novi moduli/datoteke so prihodnje delo; v tej pripravi je dodan
samo ta plan.
