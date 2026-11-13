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
| 3 | EQ 15 pasov, Advanced, urejanje presetov | čaka |
| 4 | Obdelava zvoka: pan, mono, normalizacija, tišina, SOFA | čaka |
| 5 | Prehodi in crossfade | čaka |
| 6 | Sleep timer | čaka |
| 7 | Izseki, A-B, shranjeni izseki v Bookmarks, izvoz | čaka |
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
