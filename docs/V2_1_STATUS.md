# ApricotPlayer 2.1: stanje dela

Ta dokument vodi delo za 2.1 beta 1. Obseg je potrjen v 19 sklopih (EQ, obdelava zvoka, prehodi, sleep timer, izseki, podnapisi, zvočne knjige, navigacija, playlisti, datoteke, Radio, IPTV, snemanje, pretvornik, YouTube, sistemske integracije, Spotify naročnine, bližnjice in dokumentacija). Beta 1 izide šele, ko so vsi sklopi narejeni in preverjeni.

## Pravila

- Nove funkcije so lastna implementacija. Kot referenca služi samo opisano vedenje drugih predvajalnikov, ne njihova izvorna koda.
- Javna besedila (changelog, opombe ob izdaji, README) opisujejo samo Apricotove funkcije.
- Obstoječe vedenje, vrstni red menijev in Tab vrstni red Python verzije ostanejo. Novi vnosi se dodajo za obstoječimi.
- Vsa nova besedila so v `rust_strings.json` v vseh 27 jezikih.
- Moduli, označeni z `2.1 draft`, so osnutek. Oznaka se odstrani, ko je sklop dokončan, lokaliziran in preverjen.

## Enote

| Enota | Vsebina | Stanje |
| --- | --- | --- |
| 1 | Utrditev osnutka: testi, fmt, clippy, prevodi kataloga bližnjic, vrstni red menijev in Tab | narejeno |
| 2 | Kategorije bližnjic: drevo, iskanje, filtri, primarna in dodatna bližnjica, ponastavitve | narejeno |
| 3 | EQ 15 pasov, Advanced, urejanje presetov | narejeno |
| 4 | Obdelava zvoka: pan, mono, normalizacija, tišina, SOFA | narejeno |
| 5 | Prehodi in crossfade | narejeno |
| 6 | Sleep timer | narejeno |
| 7 | Izseki, A-B, shranjeni izseki v Bookmarks, izvoz | narejeno |
| 8 | Zvočne sledi in podnapisi, glasno branje | narejeno |
| 9 | Zvočne knjige, navigacija in informacije | narejeno |
| 10 | Playlisti, mape, formati | narejeno |
| 11 | Upravljanje lokalnih datotek | narejeno |
| 12 | Radio | narejeno |
| 13 | IPTV | narejeno |
| 14 | Snemanje | narejeno |
| 15 | Pretvornik | narejeno |
| 16 | YouTube dodatki | narejeno |
| 17 | Medijske tipke in Discord | narejeno, Discord čaka na aplikacijo |
| 18 | Spotify naročnine | narejeno |
| 19 | Uvoz in izvoz, dokumentacija, izdaja | narejeno lokalno, objava čaka na Urha |

## Enota 1

Izhodišče je bil necommitan osnutek na veji `beta` nad 2.0.3. Posnetek osnutka je v lokalnem refu `refs/snapshots/gpt-2.1-draft`.

- Kontekstni meniji playlistov: "Edit playlist" je zdaj zadnji vnos, zato Python vnosi ohranijo položaje. Prazen playlist spet pokaže onemogočeno vrstico "Playlist is empty".
- Zaslon Subscriptions: gumb "Spotify subscriptions" je za Python kontrolami (Windows in skupni Shell).
- `PlaybackEvent::MediaInfo` nosi `Box`, ker so podatki o sledeh in podnapisih povečali dogodek.
- Prevodi za 91 novih ključev (katalog bližnjic, kategorije, razdelki nastavitev) v 27 jezikih.
- Testi, ki štejejo elemente, so posodobljeni za nove vnose v glavnem meniju, nastavitvah in katalogu ukazov.

## Enota 2

- Vsak ukaz ima izrecno kategorijo (`ACTION_CATEGORIES` v `shortcut_catalog.rs`); test preveri, da je vsak ukaz naveden natanko enkrat.
- Razdelek Keyboard shortcuts: Search all shortcuts, Show shortcuts, Shortcut categories, Shortcut actions, polje za bližnjico, Edit primary and alternate shortcuts, Reset selected shortcut, Clear selected shortcut, Reset category. Iskanje gre po vseh kategorijah. Tab iz seznama kategorij gre neposredno na seznam ukazov.
- Vrstica brez bližnjice pove "Unassigned". Po Reset ali Clear NVDA prebere spremenjeno vrstico, fokus ostane na gumbu.
- Nov ukaz iz 2.1 ne prevzame bližnjice, ki jo uporabnik že uporablja za drug ukaz; ostane brez bližnjice. Ukazi Python verzije ohranijo Python vedenje.
- Skupno okno obrazcev (Windows): polja se pri pomikanju samo premikajo in se nikoli ne skrijejo, Tab teče po standardnem vrstnem redu, napaka pri vnosu je cel stavek z dovoljenim razponom in fokus gre na napačno polje. Gumba Save in Cancel sta prevedena.
- Vtipkana dodatna bližnjica se shrani v standardnem zapisu (`f8` postane `F8`).

Preverjeno v izoliranem namizju z lažnim NVDA: vrstni red Tab in imena kontrol, filtriranje po kategoriji (34 ukazov v Playback), iskanje `like`, Clear z najavo, obrazec za dodatno bližnjico (fokus, Tab, Escape, vrnitev fokusa na gumb).

Ročni NVDA preizkus: Ctrl+Alt+S, End (Keyboard shortcuts), Tab. NVDA mora prebrati "Search all shortcuts, edit", nato "Show shortcuts, combo box", "Shortcut categories, list", "Shortcut actions, list". V kategorijah s puščico dol izberi Playback and sleep timer in pritisni Tab: seznam ima samo ukaze predvajanja. Tab do Clear selected shortcut in Space: NVDA prebere ime ukaza in "Unassigned".

## Enota 3

- 15 pasov: 10 obstoječih (31 Hz do 16 kHz) ostane nespremenjenih, novi 25 Hz, 100 Hz, 400 Hz, 1,6 kHz in 10 kHz začnejo ravni, zato se zvok obstoječih profilov ne spremeni. Izvoz profila vsebuje vseh 15 pasov.
- Vgrajene presete je mogoče urejati; Reset this preset vrne tovarniške vrednosti.
- Advanced (zadnji gumb v oknu F4, za Python gumbi): za vsak pas frekvenca v Hz in širina Q ter nizka in visoka polica s frekvenco, širino in ojačanjem. Spremembe se slišijo takoj, Save jih shrani za prikazani profil, Cancel vrne prejšnji zvok. Potrditveno polje vrne privzete vrednosti.
- Profil brez naprednih sprememb predvaja popolnoma enak filter kot pred 2.1. Napredne nastavitve so v `settings.json` pod `equalizer_advanced_v1`, poškodovan vnos se preskoči.
- Izvoz profila doda napredne nastavitve (`advanced`), uvoz jih prevzame. Izbris lastnega profila izbriše tudi njegove napredne nastavitve.
- Med odprtim oknom F4 zvok oblikujejo napredne nastavitve prikazanega profila, po zaprtju spet učinkovitega profila.

Preverjeno v izoliranem namizju: Tab vrstni red okna F4 (prednastavitev, razpon, 15 drsnikov od 25 Hz do 16 kHz, gumbi, Advanced zadnji), obrazec Advanced (naslov s profilom, 37 polj, Save shrani v `settings.json`, najava "Advanced equalizer settings saved.", fokus nazaj na Advanced). Veriga filtrov preverjena s ffmpeg.

Ročni NVDA preizkus: predvajaj datoteko, F4. Tab gre čez preset, razpon in 15 drsnikov ("Equalizer 25 Hz deep bass" do "Equalizer 16 kHz air and sparkle"), nato gumbe; zadnji je Advanced. Space na Advanced odpre "Advanced equalizer: ...". Spremeni "High shelf: gain in dB" na 6 in pritisni Enter: slišiš svetlejši zvok, NVDA pove "Advanced equalizer settings saved.", fokus je na Advanced.

## Enota 4

- Eno okno Audio processing (F5 v predvajalniku, zadnji vnos kontekstnega menija predvajalnika, Settings > Playback > Audio processing): stereo ravnovesje in korak, mono, dinamična normalizacija z omejevalnikom (privzeto izključena, ločeno od ReplayGain), odstranjevanje tišine z vsemi nastavitvami, prostorski zvok v slušalkah z datoteko SOFA (polje in gumb Browse), ojačanje, vrtenje, višina in korak.
- Iz predvajalnika se nastavitve shranijo takoj in slišijo takoj. Iz Settings ostanejo v neshranjenih nastavitvah do Save; po Save se filter v predvajalniku zamenja.
- Obdelava je v začetni verigi filtrov vsakega predvajanja (za hitrostjo, EQ in višino tona), zato se sliši od prve sekunde, ne šele po zagonu.
- Alt+Shift+Left in Right premikata ravnovesje ("Balance 5 right", "Balance centre"), Alt+Shift+V ga pove. Ctrl+Alt+Shift+puščice vrtijo in dvigujejo prostorski zvok; ko je izklopljen, NVDA pove "Headphone spatialization is off."
- Normalizacija gleda naprej približno 0,7 s, zato zagon in premikanje ostaneta odzivna.
- Napake so prevedeni stavki (manjkajoča datoteka SOFA, vrednost zunaj razpona).

Preverjeno v izoliranem namizju: imena in vrstni red vseh 20 polj in gumba Browse, shranjevanje (najava "Audio processing saved."), ravnovesje s tipkami, shranjene nastavitve v `settings.json` in ponovni zagon z obdelavo v začetni verigi brez napak v mpv.log. Prostorskega zvoka nisem mogel preizkusiti brez datoteke SOFA.

Ročni NVDA preizkus: predvajaj datoteko, F5. NVDA prebere "Audio processing" in "Stereo balance: -100 left, 0 centre, 100 right, edit". Označi Play as mono in pritisni Enter: NVDA pove "Audio processing saved.", zvok je mono. Alt+Shift+Right trikrat: "Balance 15 right".

## Enota 5

- Okno Playback transitions (kontekstni meni predvajalnika, Settings > Playback): crossfade samodejnih prehodov (privzeto izključen, 3 s, do 30 s), crossfade pri Previous/Next (privzeto izključen), krivulja, pojemanje pri premikanju, premoru, začetku in koncu datoteke. Gapless ostane obstoječa Python nastavitev (osnutek jo je prepisoval, popravljeno).
- Naslednji posnetek se pri vključenem crossfadu začne pred koncem trenutnega; runtime pusti stari predvajalnik teči in prekriža glasnost. Pojemanje na koncu datoteke je v runtime.
- Runtime test s FakeEngine potrdi, da stari predvajalnik teče, dokler se glasnosti ne prekrižata.
- Preverjeno v izoliranem namizju: playlist z dvema 12-sekundnima datotekama, crossfade 3 s, Autoplay next vključen: drugi posnetek se je začel pri 8,9 s. Spotify crossfade (osnutek v predvajanju Spotify) ni preverjen.

Ročni NVDA preizkus: v kontekstnem meniju predvajalnika izberi Playback transitions, označi "Crossfade when the next item starts by itself" in Save (NVDA: "Playback transitions saved."). Predvajaj playlist z vključenim Autoplay next: naslednji posnetek se začne 3 s pred koncem, zvoka se prelijeta.

## Enota 6

- Sleep timer v glavnem meniju (za Copy diagnostic report, pred Settings), Ctrl+Alt+T odpre okno, Alt+Shift+T pove preostali čas, Ctrl+Alt+Shift+T prekliče.
- Okno: najprej Current timer (stanje), nato Run the sleep timer, When to finish (čas, konec posnetka, konec poglavja), Duration (15 do 120 minut ali lastno), Count down (dejanski čas s premori ali samo predvajanje), When the timer finishes (pavza s položajem ali stop), Fade out before finishing (privzeto izključeno) in trajanje. Gumb je Start.
- Zadnje izbire se zapomnijo, tekoče odštevanje se po ponovnem zagonu ne nadaljuje. Ročna menjava posnetka časovnika do konca posnetka ne sproži in ne prekliče.
- Opozorilo minuto pred koncem, razen pri časovnikih do 75 s. Vse najave so prevedene.

Preverjeno v izoliranem namizju: vnos v glavnem meniju, Tab vrstni red in imena vseh polj, začetek z eno minuto ("Sleep timer: 1:00 remaining."), Alt+Shift+T ("0:58 remaining"), preklic in ponovni preklic ("No sleep timer is running.").

Ročni NVDA preizkus: Ctrl+Alt+T. NVDA prebere "Sleep timer" in "Current timer, No sleep timer is running." Tab do Duration, izberi 15 minutes, Enter: NVDA pove "Sleep timer: 15:00 remaining." Alt+Shift+T pove preostali čas, Ctrl+Alt+Shift+T "Sleep timer cancelled."

## Enota 7

- [ in ] ostaneta oznaki začetka in konca, P ostane enkratni predogled (Python). Alt+L vklopi in izklopi ponavljanje A-B označenega izseka; premikanje je med ponavljanjem omejeno na izsek.
- Alt+B odpre Marked excerpts: izsek, dejanje (Play once, Repeat, Save in Bookmarks, Export this excerpt, Export all excerpts, Delete, Clear all), natančen začetni in končni čas z milisekundami, ime zaznamka, potrditveno polje za uporabo trenutnih učinkov pri izvozu. Alt+Shift+PageUp in PageDown skočita na prejšnji ali naslednji izsek in ga predvajata enkrat.
- Izvoz uporablja obstoječi Python izvoz označenega posnetka (mapa clips, nastavljena oblika), privzeto z izvornim zvokom. Z vključenim poljem se dodajo hitrost, višina tona, EQ in obdelava zvoka kot filter FFmpeg.
- Shranjeni izsek je v Bookmarks z oznako "Excerpt until 0:04". Enter ga predvaja enkrat in na koncu ustavi (Windows in Mac).

Preverjeno v izoliranem namizju: imena in vrstni red polj, izvoz (najavi "Clip export started." in "Clip exported: song - 0-02-0-04.mp3", datoteka 96 kB), shranjen zaznamek z `end_position`, Alt+L vklop in izklop.

Ročni NVDA preizkus: predvajaj datoteko, pritisni [ in čez nekaj sekund ]. Alt+B: NVDA prebere "Marked excerpts" in "Excerpt, combo box, Excerpt 1: ...". Action Save in Bookmarks, Enter: "Excerpt saved in Bookmarks." Ctrl+Shift+K: vrstica vsebuje "Excerpt until". Enter predvaja izsek enkrat in ustavi.

## Enota 8

- Alt+A: izbira zvočne sledi (jezik, naslov, kodek, zunanja datoteka), Alt+PageDown in Alt+PageUp krožita med sledmi in povesta izbrano. Če datoteka nima druge sledi, NVDA to pove.
- Alt+S Subtitles: sled podnapisov ali izklopljeno, glasno branje, nova vrstica prekine prejšnjo (privzeto izključeno, vrstica se prebere do konca), ohranjanje jezika do zaprtja predvajalnika (privzeto vključeno). Zadnji dve izbiri se shranita.
- Alt+Shift+S naloži datoteko s podnapisi, Ctrl+Alt+Shift+F8 podnapise s spletnega naslova; po nalaganju se berejo na glas.
- Najave za način zvočne knjige in ustavitev na koncu poglavja so prevedene.

Preverjeno v izoliranem namizju z datoteko z dvema zvočnima sledema in podnapisi: seznam sledi ("Track 2: slv, Commentary, opus"), preklop z Alt+PageDown, izbira podnapisov in branje obeh vrstic brez prekinjanja.

Ročni NVDA preizkus: predvajaj video z več sledmi, Alt+PageDown pove novo sled. Alt+S, izberi podnapise in označi Read subtitles aloud, Enter: NVDA bere vrstice podnapisov.

## Enota 9

- M4B s poglavji samodejno vklopi način zvočne knjige, Alt+K ga ročno vklopi ali izklopi. V tem načinu Ctrl+PageUp in Ctrl+PageDown premikata po poglavjih z enako najavo kot Alt+puščice, T in Alt+T, Alt+R, Alt+D povedo čas znotraj poglavja, Ctrl+T in Alt+P skočita znotraj poglavja. Položaj za nadaljevanje in zaznamke ostane v celotni knjigi. Alt+Shift+C ustavi na koncu poglavja.
- Ctrl+G skok na številko posnetka, Ctrl+Alt+PageUp in PageDown prvi in zadnji posnetek seznama, Alt+Shift+I pove številko, skupni, predvajani in preostali čas ter velikost lokalnih datotek.
- Alt+T, Alt+R, Alt+D, Alt+Shift+P, Alt+N, Alt+I povedo pretečeni, preostali čas, trajanje, odstotek, naslov in izvajalca.
- Deset hitrih položajev na datoteko: Alt+Shift+1 do 0 nastavi, Alt+1 do 0 skoči ("Quick position 1: 0:24"). Shranjeni so v `settings.json` (`bookmark_slots_v1`).
- Playback transitions ima možnost "After the last item, start again from the first".

Preverjeno v izoliranem namizju z M4B s tremi poglavji: premik na naslednje poglavje ("Chapter Two. 0:20."), čas znotraj poglavja, hitri položaj nastavi in skoči, prazen položaj, Ctrl+G zunaj seznama, vklop in izklop načina zvočne knjige.

Ročni NVDA preizkus: odpri M4B s poglavji, Ctrl+PageDown: NVDA pove ime naslednjega poglavja. T pove čas znotraj poglavja. Alt+Shift+1, nato kasneje Alt+1: NVDA pove "Quick position 1: ..." in skoči nazaj.

## Enota 10

- Urejevalnik playlista (Alt+E na playlistu ali v njegovih vnosih) je en dialog z imenom, opisom, seznamom vnosov in izbiro dejanja. Vse spremembe so na kopiji in se zapišejo šele z "Save playlist and close"; Escape zavrže vse.
- Dejanja: dodaj datoteko, spletno povezavo ali mapo, uvozi M3U, M3U8, PLS, XSPF, ASX ali CUE, izvozi v M3U8, M3U ali JSON, označi ali odznači vnos, označi vse, počisti oznake, premakni in odstrani označene ali izbrani vnos.
- Mapo lahko dodaš kot živo mapo (vnos "Folder: pot", datoteke se preberejo ob vsakem odprtju playlista) ali kot trenutne datoteke. Izvoz v M3U8 mapo razširi v datoteke, JSON jo ohrani kot mapo.
- Po vsakem dejanju se dialog vrne na seznam vnosov z istim dejanjem izbranim, zato Enter ponovi dejanje na naslednjem vnosu. Za to ima skupni obrazec novo polje `initial_focus`, ki ga upoštevata Windows in macOS.
- Ctrl+Alt+Shift+F2 shrani trenutni seznam predvajanja kot nov playlist; obstoječe ime javi "A playlist with that name already exists."

Preverjeno v izoliranem namizju: shranjevanje trenutnega seznama, odprtje urejevalnika, vrstni red Tab (ime, opis, vnosi, dejanje, Do it, Cancel), izvoz skozi pravi Windows dialog v M3U8, označevanje ("1. Marked. 01-alpha"), premik, shranitev v `playlists.json` in najava "Playlist saved."

Ročni NVDA preizkus: predvajaj datoteko, Ctrl+Alt+Shift+F2, vpiši ime, Enter: NVDA pove "The playback list was saved as a playlist." Ctrl+Alt+P, Alt+E: NVDA pove "Edit playlist" in ime playlista. Tab do Action, izberi "Mark or unmark the selected entry", Enter: fokus je na Entries in NVDA prebere "1. Marked. ...". Tab do Action, "Save playlist and close", Enter: NVDA pove "Playlist saved."

## Enota 11

- V seznamu mape (Ctrl+Alt+O) Delete premakne izbrano datoteko v koš (Windows Recycle Bin, macOS Trash) brez vprašanja, ker jo je mogoče obnoviti. Shift+Delete jo trajno izbriše šele po potrditvi, privzeti gumb je No. Odstranjevanje iz zgodovine, priljubljenih in playlistov ostaja samo odstranjevanje vnosa.
- Alt+Shift+M odpre Manage files: seznam datotek z oznakami in dejanja premakni v koš, trajno izbriši, kopiraj v mapo, premakni v mapo, označi, označi vse, počisti oznake. Dejanje velja za označene datoteke ali za izbrano, če ni nobene oznake.
- Ob obstoječem imenu v ciljni mapi se odpre izbira Skip, Keep both (nova datoteka dobi številko, na primer "02-beta (2).ogg") ali Replace in potrditveno polje "Do the same for the remaining files". Cancel ustavi preostanek, že premaknjene datoteke ostanejo, kjer so. Datoteka nikoli ne nadomesti same sebe.
- Premik na drug disk kopira in izvirnik izbriše šele po uspešni kopiji. Po operaciji seznam ostane na isti vrstici, najava pove rezultat ("Moved to the Recycle Bin: ...", "2 files moved."), neuspele datoteke pa so naštete v sporočilu z razlogom sistema.
- Enaki ukazi so v kontekstnem meniju datoteke v seznamu mape.

Preverjeno v izoliranem namizju s pravim košem in pravimi sistemskimi dialogi: Delete, Shift+Delete z odgovorom No, Manage files z vrstnim redom Tab (Files, Action, Do it, Close), označi vse, premik v mapo z obstoječim imenom in izbiro Keep both za vse. Testne datoteke so bile nato odstranjene iz koša.

Ročni NVDA preizkus: Ctrl+Alt+O, izberi mapo, na datoteki pritisni Delete: NVDA pove "Moved to the Recycle Bin: ime". Shift+Delete: NVDA prebere vprašanje, fokus je na No. Alt+Shift+M: NVDA pove "Manage files" in prvo datoteko; izberi Mark all files, Enter, nato Move to folder, Enter in izberi mapo z isto datoteko: NVDA prebere "ime already exists in the destination folder".

## Enota 12

- Radio (Ctrl+Alt+Shift+1 ali vnos Radio v glavnem meniju) odpre meni: iskanje postaj, vse postaje, priljubljene, nedavno predvajane, najbolj poslušane, trenutno priljubljene, z največ glasovi, nedavno poslušane pri drugih, nedavno posodobljene, države, regije, jeziki, oznake, kodeki in nastavitve radia. Meni si zapomni zadnjo izbiro.
- Seznam postaj: Enter predvaja, Ctrl+F doda ali odstrani priljubljeno, Ctrl+L kopira naslov toka, F7 pokaže podrobnosti, kontekstni meni pa ponudi še ime postaje, spletno stran, glasovanje, filtre, osvežitev, iskanje, brisanje zgodovine in nalaganje naslednjih postaj. Nalaganje naslednjih postaj postavi izbiro na prvo novo postajo. Back se vrne v meni radia.
- Postaje se predvajajo neposredno z naslovom iz imenika, brez iskanja z yt-dlp, zato ostaneta ime postaje in njena oznaka (enako velja za kanale IPTV). Nov izvajalec in naslov skladbe se najavita samodejno, vsak samo enkrat, ime toka iz naslova pa se ne najavi kot skladba. Alt+Shift+N ponovi trenutno skladbo. Samodejno najavo lahko izklopiš v nastavitvah radia.
- Napake imenika so prevedene (brez povezave, zaseden imenik, napaka strežnika, nečitljivi podatki, zavrnjen glas). Popravljena je napaka osnutka, zaradi katere je imenik vsako iskanje zavrnil (prazen parameter HTTPS).
- Vsa besedila radia in skupna besedila virov so prevedena v 27 jezikov.

Preverjeno v izoliranem namizju s pravim imenikom in pravim tokom: meni in vrstni red Tab (seznam, Open, Actions, Back), seznam 100 postaj z največ glasovi, Ctrl+F, predvajanje Radio Paradise, samodejna najava skladbe in Alt+Shift+N, zapis v `radio.json`.

Ročni NVDA preizkus: Ctrl+Alt+Shift+1: NVDA pove "Radio" in "Search stations". Puščica dol do "Most voted", Enter: po "Loading" NVDA prebere prvo postajo. Ctrl+F: "Added to radio favorites." Enter: "Playing: ime postaje", ob novi skladbi NVDA prebere izvajalca in naslov. Alt+Shift+N ponovi skladbo.

## Enota 13

- IPTV (Ctrl+Alt+Shift+2 ali vnos IPTV v glavnem meniju) odpre seznam virov. Brez virov se takoj odpre obrazec za nov vir. Viri so datoteka ali naslov seznama M3U, prijava Xtream ali portal Stalker. Naslov seznama in programski vodnik imata gumb Browse, obrazec z napako se vrne z vpisanimi podatki. Prijava je shranjena z zaščito sistema.
- Navigacija: vrsta vsebine (TV v živo, filmi, serije), nato kategorija s številom kanalov, nato kanali. Zaslon, kjer ni izbire, se preskoči. Back gre vedno korak nazaj do zaslona z izbiro, zasloni pa tečejo v eni zanki, zato se klici ne gnezdijo. Serija odpre seznam epizod ("Season 1, episode 2: naslov"), Back se vrne na serije.
- Vrstica kanala pove ime in trenutno oddajo ("Paradise, now: Morning show"). Ob predvajanju NVDA najprej pove "Playing: ime kanala", nato "Now: oddaja", če jo vodnik pozna. Alt+Shift+N pove trenutno in naslednjo oddajo s časom, podrobnosti pa so v kontekstnem meniju.
- Naslovi ponudnikov Xtream in Stalker vsebujejo prijavo, zato se taki kanali ne zapišejo v zgodovino in ne v položaje za nadaljevanje, kopiranje naslova pa je ponujeno samo za kanale iz seznamov.
- Popravljena napaka osnutka v sistemu Windows: ko je seznam iz ozadja začel novo delo (na primer predvajanje kanala ali glas za postajo), se rezultat ni nikoli obdelal, ker se je časovnik ustavil.
- Vse napake IPTV in vsa besedila so prevedena v 27 jezikov.

Preverjeno v izoliranem namizju z lokalnim seznamom M3U, vodnikom XMLTV in pravim tokom: seznam virov, kategorije s števili, kanal z oddajo, Back s kanalov na kategorije, predvajanje z vrstnim redom najav in Alt+Shift+N. Xtream in Stalker sta preverjena samo s testi, ker za njiju ni testnega računa.

Ročni NVDA preizkus: dodaj vir s seznamom M3U, Ctrl+Alt+Shift+2, Enter: NVDA prebere kategorije s števili. Izberi kategorijo, Enter: NVDA prebere kanal in trenutno oddajo. Enter: "Playing: kanal", nato "Now: oddaja". Alt+Shift+N: trenutna in naslednja oddaja s časom.

## Enota 14

- Snemanje (Ctrl+Alt+Shift+3 ali vnos Recording v glavnem meniju) odpre okno s stanjem ("Not recording", "Recording since 14:02", "Recording paused") in seznamom virov z glasnostjo. Viri so mikrofoni in vhodi, zvok računalnika po napravah, posamezne aplikacije in "vse razen aplikacije". Escape zapre samo okno, snemanje teče naprej.
- Ctrl+Alt+F9 začne snemati. Če vir še ni izbran, se najprej odpre izbira vira in snemanje se začne takoj po izbiri. Ctrl+Alt+F10 začasno ustavi ali nadaljuje, Ctrl+Alt+F11 ustavi, Ctrl+Alt+Shift+F10 odpre mapo s posnetki. Vsaka sprememba se najavi samo enkrat (osnutek je premor najavil dvakrat).
- Privzeto FLAC, potrditev z ločenima zvokoma za začetek in konec, govor ali oboje po izbiri. "Remember this choice" shrani vire in glasnost za naslednji zagon, iste nastavitve odpre tudi Settings, Recording. Datoteka se piše v začasno ime in se preimenuje šele, ko je posnetek dokončan, obstoječa datoteka se nikoli ne prepiše.
- Vse napake in besedila snemanja so prevedeni v 27 jezikov. Ker so Radio, IPTV in snemanje zdaj končani, so moduli virov v celoti preverjeni s strogim lintom (brez izjeme osnutka), navigacija radia pa teče v zanki kot pri IPTV.

Preverjeno v izoliranem namizju s pravimi napravami: Ctrl+Alt+F9 brez virov odpre seznam (mikrofoni, zvok računalnika, aplikacije), izbira zvoka računalnika takoj začne snemanje, okno pokaže stanje in vir, premor in nadaljevanje, ustavitev in shranjena datoteka FLAC. Zajem aplikacije in macOS sta preverjena samo s prevajanjem in testi.

Ročni NVDA preizkus: Ctrl+Alt+F9: NVDA prebere seznam virov. Izberi "Computer audio: ...", Enter: zasliši se zvok začetka. Ctrl+Alt+Shift+3: prva vrstica pove "Recording since ...". Escape, Ctrl+Alt+F10: "Recording paused.", znova: "Recording resumed.", Ctrl+Alt+F11: "Finishing the recording." in zvok konca.

## Enota 15

- Nova ciljna formata MKA (zvok se prekopira brez izgube, ob spremembi frekvence ali kanalov pa se kodira v FLAC) in OGA (FLAC v Ogg). Vsi obstoječi formati za zvok, video in sliko ostanejo.
- Converter options (Settings, Downloads ali Ctrl+Alt+Shift+K): bitna hitrost za MP3, M4A in AAC, Opus ter WMA, AC3 in MP2, namen Opus (glasba, govor, majhna zakasnitev), kakovost Ogg Vorbis, stiskanje FLAC, frekvenca vzorčenja in kanali (privzeto ohrani izvirno), število hkrati pretvorjenih datotek mape od 1 do 8 in kam gre pretvorjena mapa (izbrana mapa ali poleg izvirnikov). Privzete vrednosti dajo natanko ukaze Python verzije.
- Pretvarjanje mape teče vzporedno, imena izhodnih datotek pa se določijo vnaprej, zato se dve hkratni datoteki nikoli ne zapišeta pod isto ime. Okno napredka ima gumb Cancel (tudi Escape), ki ga NVDA prebere ob odprtju. Cancel ustavi tekoči FFmpeg in ne začne novih datotek, ustavljena datoteka ne pusti delnega izhoda. Ctrl+Alt+Shift+X ustavi pretvarjanje tudi brez okna.
- Na koncu je za vsako neuspelo datoteko izpisan razlog ("These files were not converted:"), po preklicu pa "Conversion stopped. N files were converted."

Preverjeno v izoliranem namizju s pravim FFmpeg: pretvornik mape ponudi MKA in OGA, okno napredka s fokusom na Cancel, dve hkratni pretvorbi in datoteke MKA poleg izvirnikov. Preklic in vzporedna imena sta preverjena s testi.

Ročni NVDA preizkus: v glavnem meniju Folder converter, izberi mapo in MKA, Convert: NVDA prebere "Converting folder", besedilo napredka in gumb Cancel. Escape: "Stopping the conversion.", nato "Conversion stopped. ...". Ctrl+Alt+Shift+K odpre možnosti pretvornika.

## Enota 16

- Predlogi za iskanje: v polju za iskanje Ctrl+Shift+Down naloži predloge YouTube za vpisano besedilo (v jeziku vmesnika) in jih ponudi v seznamu. Izbrani predlog se vpiše v polje in iskanje se začne takoj. Nalaganje teče v ozadju, brez predlogov se najavi "No suggestions for this query."
- Izbira povezave: povezava na videoposnetek v seznamu predvajanja (watch?v=...&list=... ali youtu.be/...?list=...) vpraša "Only the video" ali "The whole playlist". Videoposnetek se predvaja ali prenese sam, seznam se odpre kot seznam videoposnetkov ali prenese v celoti.
- Prenos z možnostmi (Alt+Shift+D na izbranem ali predvajanem elementu): zvok ali video, oblika in kakovost zvoka, oblika videa, največja višina in mapa samo za ta prenos. Shranijo se samo razlike od nastavitev, ki ostanejo na elementu tudi v čakalni vrsti. Obstoječa kakovost, piškotki in čakalna vrsta ostanejo enaki.
- Posebnih možnosti predvajanja posameznega elementa nisem dodal, ker jih Python verzija nima in plan ne pove, kaj naj bi bile; izbira "Only the video" pokrije primer predvajanja enega videoposnetka iz seznama.

Preverjeno v izoliranem namizju s pravim omrežjem: predlogi za "mozart", izbira tretjega predloga in rezultati iskanja zanj, obrazec za prenos z možnostmi z imeni vseh polj, vprašanje za povezavo s seznamom.

Ročni NVDA preizkus: Ctrl+Alt+Y, vpiši "mozart", Ctrl+Shift+Down: NVDA pove "Loading suggestions." in nato "Search suggestions" s prvim predlogom. Puščice, Enter: iskanje teče. Na rezultatu Alt+Shift+D: NVDA prebere "Download with options" in "Download as". Ctrl+Alt+L, prilepi povezavo z &list=, Enter: NVDA prebere "This link has a video and a playlist".

## Enota 17

- Medijske tipke in sistemski predstavnostni kontrolniki (Windows: prekrivni kontrolnik z naslovom in izvajalcem, macOS: Now Playing) so privzeto vklopljeni in jih lahko izklopiš. Predvajaj ali premor, naslednji, prejšnji, previjanje in položaj delujejo kot tipke predvajalnika, vendar samo, ko je predvajalnik odprt. Premik drsnika položaja v sistemskem kontrolniku ne povzroči najave.
- Discord Rich Presence je privzeto izklopljen. Ko ga vklopiš, se privzeto delijo naslov, izvajalec ali kanal, slika in čas predvajanja, vsako od tega lahko izklopiš, med premorom pa se privzeto ne prikazuje. Povezava z Discordom teče v svoji niti in se ob napaki vsakih 15 sekund poskusi znova, zato nikoli ne ustavi vmesnika.
- Integracije (Ctrl+Alt+Shift+4 ali Settings, General, "Media controls and Discord") so en obrazec z vsemi stikali. Discord potrebuje svojo aplikacijo Discord z ID-jem, ki je ApricotPlayer še nima, zato obrazec pove, da bo deljenje na voljo pozneje, izbire pa se shranijo.

Preverjeno: testi preslikave medijskih dogodkov, zasebnosti Discorda in obrazca, obrazec Integrations v izoliranem namizju z imeni vseh stikal. Sistemskih kontrolnikov v izoliranem namizju ni mogoče preveriti, ker jih Windows tam ne prikaže; preveriti jih je treba ročno na navadnem namizju.

Ročni NVDA preizkus: predvajaj datoteko, pritisni tipko za predvajanje ali premor na tipkovnici: NVDA pove "Paused" ali "Playing". Win+A ali prekrivni kontrolnik glasnosti pokaže naslov skladbe. Ctrl+Alt+Shift+4: NVDA prebere "Media controls and Discord" in prvo stikalo.

## Enota 18

- Naročnine na sezname Spotify (Ctrl+Alt+Shift+B ali meni Spotify) so zdaj v celoti lokalizirane: plošča, obrazci za možnosti, interval in naročanje s povezavo, sporočila o uspehu in vse napake.
- Preverjanje teče samodejno vsakih 5 minut glede na shranjeni interval (privzeto ena ura) in samo za račun, ki je izbran v meniju Spotify. Ostali računi obdržijo svoje naročnine. Samodejno preverjanje ne pove ničesar, če ni sprememb; "Check now" na koncu pove "Spotify subscriptions checked."
- Privzeto se oglasijo samo dodane skladbe, odstranjene skladbe in spremenjen vrstni red se vklopijo posebej za vsak seznam. Povzetek je eno sporočilo v obliki "ime: 3 songs added, 1 removed", seznam sprememb pa se odpre kot svoj seznam.
- Na lastne sezname in sezname neznanega lastnika se ni mogoče naročiti.

Preverjeno: testi besedil plošče, povzetka in poročil, testi ločenih računov in nespremenjenih poškodovanih podatkov. V živo nisem preverjal, ker potrebuje prijavljen račun Spotify.

Ročni NVDA preizkus: prijavi se v Spotify, Ctrl+Alt+Shift+B: NVDA prebere "Spotify playlist subscriptions". Izberi "Subscribe using a link", prilepi povezavo na tuj seznam, Enter: NVDA pove "Subscribed to the playlist." Ponovno odpri ploščo in izberi "Check now": NVDA pove "Updating Spotify subscriptions." in nato "Spotify subscriptions checked."

## Enota 19

- Export settings (Ctrl+Alt+Shift+F6) shrani shranjene nastavitve v datoteko v obliki `settings.json`, zato jo prebereta obe verziji. Import settings (Ctrl+Alt+Shift+F5) najprej vpraša, nato zamenja vse nastavitve, staro datoteko pa obdrži kot `settings.json.before-import`. Datoteka, ki ni varnostna kopija nastavitev, ne spremeni ničesar. Uvoz deluje tudi, ko trenutnih nastavitev ni bilo mogoče prebrati.
- Export bookmarks (Ctrl+Alt+Shift+F4) in Import bookmarks (Ctrl+Alt+Shift+F3). Uvoz privzeto doda nove zaznamke in preskoči obstoječe. "Replace all my bookmarks with them" je ločena izbira, ki vpraša s številom trenutnih zaznamkov in obdrži kopijo. Poškodovan vnos ustavi uvoz z njegovo številko.
- Vsi štirje ukazi so tudi na koncu Settings, General. Skupna logika je v `apricot-shell/src/backups.rs`, Windows ovoj v `backups_win32.rs`, macOS jo uporablja prek skupnega Shell.
- Odstranjen je ukaz osnutka "local_file_actions" (Alt+F), ki ni imel nobenega vedenja.
- Vsi moduli, ki se gradijo na Windows, so zdaj v celoti preverjeni s strogim lintom. Oznako osnutka imajo samo še trije moduli za macOS (`recording_macos.rs`, `feature_form.rs` in `sources_browser.rs` v macOS UI), ker jih na Windows ni mogoče preveriti.
- Verzija je 2.1.0-beta.1, changelog, README in `release-notes/v2.1.0-beta.1.md` so posodobljeni.

Preverjeno v izoliranem namizju: izvoz nastavitev in zaznamkov, uvoz zaznamkov z združevanjem ("No new bookmarks; all of them are already saved."), uvoz nastavitev z odgovorom No (nič se ne spremeni) in Yes (glasnost iz kopije, kopija pred uvozom obstaja), fokus ostane na glavnem meniju.

Ročni NVDA preizkus: Ctrl+Alt+Shift+F6, shrani datoteko: NVDA pove "Settings exported." Ctrl+Alt+Shift+F3, izberi isto datoteko z zaznamki: NVDA prebere "Import bookmarks" in "Imported bookmarks, Add them to my current bookmarks". Enter: NVDA pove rezultat.

Še odprto: objava na GitHubu in gradnja za macOS (GitHub Actions) čakata na Urhovo dovoljenje, Discord čaka na ID aplikacije, macOS koda ni preverjena na Macu.
