# Vodič za testiranje Spotify v ApricotPlayerju 2.0

Verzija: 2.0.0-beta.11 (nameščena v C:\Program Files\ApricotPlayer).
Tvoj ApricotPlayer je v angleščini, zato so imena v aplikaciji napisana v angleščini. Vse se dela s tipkovnico. Pri vsakem koraku piše, kaj mora NVDA prebrati.
Če se kaj obnaša drugače, mi napiši številko koraka in kaj si slišal.

Nekateri koraki spremenijo tvoj Spotify račun. Ob koncu jih razveljavi, kot
piše pri koraku.

## 1. Vstop, prijava in računi

1. V glavnem meniju izberi Spotify ali pritisni Ctrl+Alt+C. NVDA prebere seznam
   Spotify z vnosi Search, My library, Liked Songs, Playlists, Home, Daily
   Mixes, Recently played, Your top tracks and artists, Browse categories,
   Spotify queue, Spotify devices, Spotify settings in Spotify accounts. Ob vnosih z bližnjico NVDA prebere tudi bližnjico.
2. Ctrl+Alt+Shift+C odpre Spotify račune. Tvoj račun se prebere kot ime,
   Premium in "active". Zadnja vrstica je Add account.
3. Na računu pritisni tipko Applications. Meni ponudi Log out in Remove account.
   Ne izberi ničesar, Escape.
4. Escape gre iz računov nazaj v Spotify meni, še en Escape v glavni meni.

## 2. Iskanje in seznami

1. Ctrl+Alt+Shift+Y odpre dialog Search Spotify. Fokus je v polju Search for,
   Tab gre na Type, nato na Search in Cancel.
2. Napiši na primer queen, pusti vrsto All in pritisni Enter. NVDA prebere
   prvo vrstico, na primer "Bohemian Rhapsody, track, Queen, 5:55". Vsaka
   vrstica pove, kaj je (track, artist, album, playlist, podcast, episode).
3. Pojdi na vrstico z izvajalcem in pritisni Enter. Odpre se izvajalec:
   najprej priljubljene skladbe, nato albumi, singli in sorodni izvajalci.
4. Enter na albumu odpre album. Escape te vrne na izvajalca na isto vrstico,
   še en Escape na rezultate iskanja.
5. Ctrl+Alt+Shift+L odpre My library, Ctrl+Alt+Shift+F Liked Songs,
   Ctrl+Alt+Shift+P Playlists. Ko s puščico dol prideš na zadnjo vrstico
   Liked Songs, NVDA reče "Loading more results." in pride
   naslednja stran.
6. V seznamu predvajanja izberi peto skladbo in pritisni Enter. Predvaja se
   točno ta skladba, nato sledijo naslednje iz seznama. Escape v predvajalniku
   te vrne v isti seznam na peto vrstico.

## 3. Predvajanje in predvajalnik

1. Med predvajanjem Spotify skladbe pritisni T. NVDA prebere čas in dolžino.
2. Ctrl+PageDown preskoči na naslednjo skladbo, NVDA enkrat prebere
   "Playing:" in ime. Ctrl+PageUp v prvih treh sekundah gre na prejšnjo,
   kasneje začne isto skladbo znova.
3. Shift+S: "Shuffle on." in "Shuffle off.". R trikrat: "Repeat album or
   playlist.", "Repeat track.", "Repeat off.".
4. Levo in Desno previjata, Space ustavi in nadaljuje, gor in dol spreminjata
   glasnost, S, D in F4 delujejo kot pri drugih virih (hitrost, višina,
   izenačevalnik).
5. F v predvajalniku pove format: Ogg Vorbis in kakovost.
6. Ctrl+Shift+Y odpre besedilo. Prva vrstica je "Spotify lyrics" in ime
   ponudnika, nato besedilo pesmi.
7. Med predvajanjem zapri aplikacijo. Zapre se takoj.

## 4. Všečki, knjižnica in seznami predvajanja (spremeni račun)

1. V iskanju izberi skladbo in pritisni Ctrl+Shift+I. NVDA prebere "Added to
   Liked Songs.", vrstica dobi oznako "liked". Še enkrat Ctrl+Shift+I:
   "Removed from Liked Songs.". Preveri v uradnem Spotifyju na telefonu, da je
   bila skladba vmes všečkana.
2. Ctrl+Alt+Shift+P, nato Ctrl+Shift+N. V dialog napiši Test ApricotPlayer in
   Enter. NVDA prebere "Playlist created: Test ApricotPlayer.", seznam je na
   vrhu. Ime dialoga je Create Spotify playlist.
3. V iskanju na skladbi pritisni tipko Applications in izberi Add to Spotify
   playlist. Izberi Test ApricotPlayer. NVDA prebere "Added to Test
   ApricotPlayer.". Ponovi z drugo skladbo.
4. Odpri Test ApricotPlayer. Na prvi skladbi v kontekstnem meniju izberi
   Move down: "Playlist updated.". Na skladbi pritisni Delete: "Removed from
   this playlist.".
5. V Playlists na Test ApricotPlayer v kontekstnem meniju izberi Rename
   playlist, vpiši novo ime: "Playlist renamed".
6. Pospravi: na seznamu pritisni Ctrl+Shift+I: "Removed from your library.".
7. V Daily Mixu izberi skladbo in pritisni Ctrl+Shift+H: "Song hidden.",
   vrstica dobi "hidden". Še enkrat Ctrl+Shift+H: "Song shown again.". Drugje,
   na primer v Liked Songs, Ctrl+Shift+H samo pove, da deluje v osebnih
   miksih.

## 5. Osebne zbirke

1. Ctrl+Alt+Shift+M odpre Daily Mixes (vseh šest), Enter odpre mix.
2. V Spotify meniju odpri Home. Vsaka vrstica je razdelek z imenom in
   številom elementov, Enter ga odpre.
3. Odpri Recently played, Your top tracks and artists (tri obdobja, skladbe in
   izvajalci) in Browse categories (kategorija Music ima razdelke).
4. Na skladbi pritisni Ctrl+Alt+Shift+R. Odpre se Spotifyjev radio te
   skladbe, Enter ga predvaja.

## 6. Spotify vrsta in naprave (s telefonom)

1. Predvajaj album v ApricotPlayerju. Na skladbi pritisni Ctrl+Shift+Q:
   "Added to Spotify queue.".
2. Ctrl+Alt+Shift+Q odpre Spotify queue. Prva vrstica je "Now playing", ročno
   dodane skladbe se končajo z "added manually", naslednje iz albuma z "next
   from the album or playlist". Delete odstrani, kontekstni meni ponudi
   Move up, Move down in Clear manually added tracks.
3. Na telefonu dodaj skladbo v vrsto, medtem ko je dialog odprt. Seznam se
   posodobi, fokus ostane v seznamu.
4. Ctrl+Alt+Shift+O odpre Spotify devices. Prvi je ta računalnik z oznako
   "playing". Ko je telefon odprt, je pod njim. Enter na telefonu prenese
   predvajanje: "Playback moves to" in ime telefona.
5. Na telefonu izberi napravo ApricotPlayer. Predvajanje se vrne, NVDA
   prebere "Playing:".
6. Na telefonu premakni glasnost. V ApricotPlayerju pritisni V, glasnost je
   podobna, NVDA ob spremembi ne reče ničesar. Puščica dol v ApricotPlayerju
   premakne drsnik na telefonu.
7. Med predvajanjem drugega vira v ApricotPlayerju (YouTube ali lokalna
   datoteka) na telefonu predvajaj na ApricotPlayer. Prejšnji vir se ustavi in
   shrani položaj, fokus ostane, kjer je.

## 7. Apricotove zbirke in nastavitve

1. V iskanju na skladbi pritisni Ctrl+F: "Added to favorites.". Enako na
   albumu. Ctrl+Alt+F odpre priljubljene, Enter na skladbi jo predvaja od
   začetka, Enter na albumu predvaja cel album.
2. Skladbo lahko s Ctrl+P dodaš tudi na Apricotov seznam predvajanja.
3. V Spotify meniju odpri Spotify settings. Polja so Streaming quality,
   Normalize volume in When an album or playlist ends. Escape
   prekliče.

## 8. Napake

1. Prilepi v Direct link povezavo do izvajalca na Spotifyju in pritisni Enter.
   NVDA pove, katere povezave lahko predvaja.
2. Če Spotify ni povezan (na primer brez interneta), Spotify seznami povejo
   "Spotify is not connected" ali kratko napako omrežja, nič se ne zatakne.
