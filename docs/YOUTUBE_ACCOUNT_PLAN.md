# YouTube račun v ApricotPlayerju: primerjava s Klango 4 in plan

Stanje 26. 11. 2026. Viri: nameščen Klango 4.0.0b1 (`C:\Program Files\Klango 4`), njegove
angleške prevodne datoteke in imena funkcij v prevedenih modulih ter stran klango.online.
Koda Klanga ni javna (stran ga opisuje kot zasebno reimplementacijo, licence ni), zato je
Klango samo vzor za vedenje. Kode iz njega ne prevzemamo.

## 1. Kaj ima Klango za YouTube

- **Prijava z Google računom**: Klango odpre svoj brskalnik z začasnim profilom, uporabnik se
  prijavi, Klango ob prepoznani prijavi sam zapre brskalnik in si zapomni piškotke. Odjava
  piškotke pozabi. Ko YouTube prijave ne prepozna več, to pove in ponudi novo prijavo.
- **Seznami po prijavi**: Home (priporočeno zate), Subscriptions (novi videi kanalov),
  Subscribed channels, My playlists, Watch later, Liked videos, History.
- **Iskanje**: Videos, Playlists, Channels ali Everything ter potrditveno polje Also show
  Shorts. Na koncu vsakega seznama je Load more, Ctrl+R osveži raven.
- **Predvajalnik**: hitrost in stiskanje časa, snemanje zvoka (R), podatki o videu (Ctrl+I),
  črkovanje naslova, priljubljene (Ctrl+F), prenos v mapo posnetkov.
- **Tehnično**: lastni klici YouTubovega spletnega vmesnika (`/youtubei/v1/browse` in
  `search` z avtorizacijo SAPISIDHASH iz piškotkov in nadaljevalnimi žetoni), yt-dlp samo kot
  rezerva in za naslove tokov.

## 2. Kaj ima Klango za Spotify

Iskanje (vse, skladbe, izvajalci, albumi, playliste, podcasti, zvočne knjige), My library,
My playlists, pri izvajalcu Popular tracks, Albums, Biography, Similar artists in Similar
tracks, Go to album, Go to artist, nadaljevanje predvajanja. ApricotPlayer ima vse to in še
veliko več (vrsta, naprave, radio, Home, Daily Mixes, Top, Browse, profili, urejanje playlist,
naročnine, besedila, smart shuffle, vse skladbe izvajalca). Manjka samo **biografija
izvajalca** (zvočne knjige si izključil že prej).

## 3. Kaj ApricotPlayer že ima za YouTube

Iskanje (All, Video, Playlist, Channel), Trending, lastne naročnine na kanale z obvestili
(Notification center), zgodovina, priljubljene in playliste (vse lokalno, za vse vire),
komentarji, sorodni videi, prenosi, piškotki (izvoz iz brskalnika, odpiranje YouTuba v
izbranem profilu brskalnika) in ključ YouTube Data API.

Preverjeno 26. 11. 2026: tvoj `cookies.txt` je iz 14. 10. in YouTube z njim ni prijavljen.
yt-dlp 2026.11.23 ima za vse sezname po prijavi lastne izluščevalnike (`youtube:recommended`,
`youtube:subscriptions`, `youtube:history`, `youtube:watchlater`, `youtube:favorites` za
všečkane, `youtube:tab` za strani `feed/...`), a brez prijave vrnejo napako "Login details
are needed". Hitrost s prijavo bom izmeril, ko se prvič prijaviš.

## 4. Kaj manjka

1. Prijava v YouTube in odjava, z imenom računa in opozorilom ob potekli prijavi.
2. Home, Subscriptions feed, Subscribed channels, My playlists, Watch later, Liked videos in
   YouTube History.
3. Shorts v iskanju.
4. Biografija Spotify izvajalca.

## 5. Kako brez podvajanja

- **Ena prijava za vse**: prijava zapiše piškotke v obstoječi `cookies.txt`, ki ga yt-dlp že
  uporablja za iskanje, predvajanje in prenose. Ločenega mehanizma ne bo.
- **Nov zaslon My YouTube** (v glavnem meniju za YouTube): pred prijavo ima samo "Sign in
  with Google account", po prijavi pa zgornjih sedem seznamov. Vsak seznam se obnaša kot
  rezultati iskanja: Enter predvaja, Load more na koncu, enak kontekstni meni (prenos,
  priljubljene, dodaj v playlisto, odpri kanal, naroči se).
- **Naročnine**: ApricotPlayerjeve naročnine ostanejo vir za Notification center. Nov ukaz
  "Import YouTube subscriptions" doda manjkajoče kanale iz računa in nikoli ničesar ne
  odstrani. Subscriptions feed v My YouTube je YouTubov seznam novih videov.
- **Zgodovina**: ApricotPlayerjeva zgodovina ostane za vse vire. YouTube History je ločen
  seznam v My YouTube. Neobvezna nastavitev "Mark played videos as watched on YouTube"
  (yt-dlp `--mark-watched`) je privzeto izklopljena.
- **Priljubljene in všečkani**: ostanejo ločeni. Liked videos je najprej samo za branje,
  Ctrl+F še naprej dodaja v ApricotPlayerjeve priljubljene.
- **Playliste**: YouTubove playliste so najprej samo za branje, z ukazom "Copy to Apricot
  playlist".
- **Iskanje**: Shorts kot dodatna možnost iskanja, privzeto izklopljena, kot v Klangu.

## 6. Tehnična izbira

- **A, priporočam**: branje seznamov z yt-dlp in piškotki. Ustreza odločitvi "YouTube vedno
  prek yt-dlp", uporabi obstoječi ogret yt-dlp in strani po 50. Pisanje (všečkaj, naroči se
  na YouTubu, dodaj v Watch later) s tem ni mogoče.
- **B**: lastni klici YouTubovega spletnega vmesnika kot v Klangu. Hitrejše strani in pisanje
  v račun, a več vzdrževanja, ker YouTube vmesnik pogosto spreminja. Smiselno šele pozneje in
  samo za pisanje, če ga boš želel.

## 7. Delovne enote

1. **Y1 Prijava**: Sign in with Google account v posebnem profilu brskalnika (obstoječi
   mehanizem za piškotke), samodejno prepozna prijavo in izvozi piškotke, pokaže ime
   računa, Sign out pozabi piškotke, opozorilo ob potekli prijavi. Windows in Mac.
2. **Y2 My YouTube**: zaslon in sedem seznamov, strani, kontekstni meni kot pri rezultatih,
   meritev hitrosti s pravim računom.
3. **Y3 Uvoz naročnin**: Import YouTube subscriptions v ApricotPlayerjeve naročnine.
4. **Y4 Shorts v iskanju**.
5. **Y5 Neobvezno**: Mark played videos as watched on YouTube.
6. **Y6 Spotify**: biografija izvajalca kot del About na strani izvajalca.
7. **Y7 Pozneje, po odločitvi**: pisanje v YouTube račun (možnost B).

Za Y1 in Y2 se moraš enkrat prijaviti v YouTube, da lahko preverim v živo.
