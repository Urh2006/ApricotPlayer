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
| 2 | Kategorije bližnjic: drevo, iskanje, filtri, primarna in dodatna bližnjica, ponastavitve | čaka |
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
