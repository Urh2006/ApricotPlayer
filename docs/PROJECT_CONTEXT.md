# ApricotPlayer: projektni kontekst

## Trenutni posnetek

- Datum posnetka: 27. avgust 2026.
- Trenutna verzija: `1.0.14`.
- Trenutna razvojna veja: `main`.
- Stabilna 1.0.14 popravlja predvajanje z nastavljenega položaja (resume position), omogoča nemoteno hitro navigacijo (skoke naprej/nazaj) brez zatikanja ter dodaja nastavitev prednostnega formata predvajanja.
- Obstoječi 1.0 roadmap in potrjeni AudioVault obseg sta izvedena.
- Popoln macOS feature-parity port je potrjeno prihodnje delo. Implementacija se
  še ni začela; trajni plan in acceptance manifest sta v
  `docs/MACOS_PORT_PLAN.md`.

To je informativni posnetek. Če se številke razlikujejo, imajo prednost `apricot/__init__.py`, `apricot/constants.py`, Git zgodovina, `CHANGELOG.md` in zadnje release notes.

## Namen

ApricotPlayer je dostopen Windows medijski predvajalnik in downloader, zasnovan predvsem za uporabnike bralnikov zaslona. Združuje spletne in lokalne medije v tipkovnično vodljivem wxPython vmesniku.

Glavne zmožnosti vključujejo:

- YouTube in SoundCloud iskanje ter predvajanje;
- lokalne datoteke, mape, history, favorites, playliste in queue;
- prenose videa, zvoka, playlistov in kanalov;
- YouTube subscriptions ter RSS in podcast knjižnice;
- transcripts, lyrics, chapters, comments in bookmarks;
- EQ profile, bass boost, ReplayGain oziroma normalization, speed in pitch;
- background playback, resume pozicije in obnovitev zadnje seje;
- stable in beta update kanala;
- installer in portable ZIP za Windows.

## Produktna obljuba

Funkcija ni dokončana samo zato, ker deluje z miško. Dokončana je, ko jo je mogoče odkriti, uporabiti, zapreti in ponovno najti s tipkovnico in NVDA brez nepričakovanega skakanja fokusa ali podvojenih obvestil.

Playerjeva hitrost je del uporabniške izkušnje. Običajen uspešen tok ne sme dobiti dodatnega omrežnega klica, cookies poskusa, čakanja ali težkega osveževanja samo zaradi redkega fallback primera.

## Arhitektura

`wx_main.py` sestavi glavni okvir iz mixinov. Domenska logika je razdeljena pod `apricot/`, predvsem na UI, player, network, download, library, media, search, system in updater. mpv skrbi za predvajanje, yt-dlp za večino spletnega pridobivanja podatkov in prenosov, wxPython pa za dostopen Windows vmesnik.

Stari razvojni chat se je začel z monolitnim `wx_main.py`, zato njegove omembe vrstic in funkcij niso nujno več aktualne. Vedno najprej preveri trenutno modularno kodo.

## Uvožen zgodovinski kontekst

Pregledani so bili vsi trenutno dosegljivi Codex chati o projektu:

- prvotni razvoj od začetka do `0.8.49`;
- obnovitveni chat in izdaja `0.8.50`;
- nadaljevanje od `0.8.51` do `1.0.0-beta.64`;
- prekinjeni chat o neodvisnem resume času.

Prekinjeni resume problem ni odprta naloga: poznejša `1.0.0-beta.31` ga je rešila tako, da se shranjena pozicija veže na dejansko predvajani item, ne na novi item, ki ga UI ravno odpira.

Stare želje za chapters, lyrics, podcast mode, transcripts, bookmarks, comments, EQ Profiles 2.0, session restore, SoundCloud in kategorije knjižnic so že implementirane. Ne dodajaj jih v backlog samo zato, ker so v starem chatu zapisane kot prihodnje funkcije.

## Trenutni viri resnice

1. Trenutna izvorna koda in testi.
2. Git zgodovina trenutne veje.
3. `CHANGELOG.md` in release notes.
4. `docs/ROADMAP_1.0.md`, `docs/DECISIONS.md` in `docs/BACKLOG.md`.
5. Povzetki starih chatov samo kot zgodovinska razlaga.

Za macOS delo je `docs/MACOS_PORT_PLAN.md` dodatni vir resnice. Beseda parity v
tem projektu pomeni vse trenutne Windows funkcije, nastavitve, shortcute in
dostopne tipkovnične poti, ne samo osnovno predvajanje.
