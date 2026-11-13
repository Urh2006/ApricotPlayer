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
| 8 | Zvočne sledi in podnapisi, glasno branje | čaka |
| 9 | Zvočne knjige, navigacija in informacije | čaka |
| 10 | Playlisti, mape, formati | čaka |
| 11 | Upravljanje lokalnih datotek | čaka |
| 12 | Radio | čaka |
| 13 | IPTV | čaka |
| 14 | Snemanje | čaka |
| 15 | Pretvornik | čaka |
| 16 | YouTube dodatki | čaka |
| 17 | Medijske tipke in Discord | čaka |
| 18 | Spotify naročnine | čaka |
| 19 | Uvoz in izvoz, dokumentacija, izdaja | čaka |

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
