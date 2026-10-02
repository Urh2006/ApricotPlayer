# P8 dokazi: Urhove odprte točke

2. 10. 2026, verzija 2.0.0-beta.12. Urh: "skušaj vse točke narediti razen
avdioknjig, profile drugih uporabnikov pa ja, lahko. enako osveževanje
playlista ki je odprt, pri mešanju spotify in apricot playlistov pa naj ima
apricot prednost če tak playlist obstaja". Vsi živi preizkusi so tekli na
ločenem nevidnem namizju s kopijo podatkov in Urhovim računom (dovoljenje
1. 10. 2026). Vse spremembe računa so razveljavljene: sledenje profilu
Spotify je preklicano, testni seznam je odstranjen iz knjižnice, razvrščanje
je spet Recents.

## Spotifyjevi vmesniki (preverjeno s sondo)

- `GET /user-profile-view/v3/profile/{user}?playlist_limit=50&artist_limit=0&episode_limit=0&market=from_token`:
  `name`, `followers_count`, `public_playlists` (`uri`, `name`, `owner_name`,
  `owner_uri`, `is_following`), `total_public_playlists_count`.
- `.../profile/{user}/playlists?offset&limit`, `.../following`,
  `.../followers`: seznami `profiles` (`uri` profila ali izvajalca, `name`).
- PF `isFollowingUsers` (`uris`), `followUsers` in `unfollowUsers`
  (`usernames`), `searchUsers` (`/searchV2/users`).
- `GET` in `POST /playlist-permission/v1/playlist/{id}/permission/base`:
  `{revision, permissionLevel}`; spletni predvajalnik za "Make private"
  nastavi `BLOCKED`, za "Make public" `VIEWER`.
- Opis: `POST /playlist/v2/playlist/{id}/changes` z
  `UPDATE_LIST_ATTRIBUTES` (`description` ali `noValue: LIST_DESCRIPTION`)
  in `baseRevision`, kot spletni `updateDetails`.
- PF `libraryV3` `order`: `Recents`, `Recently Added`, `Alphabetical`,
  `Creator`, `Custom Order` (`availableSortOrders`). Po abecedi pridejo prve
  Spotifyjeve postavke brez imena (potekli miksi); Apricot jih izpusti.
- `GET /transcript-read-along/v2/episode/{id}?format=json`: `section` z
  `title.title` (govorec), `text.sentence.text` in `startMs`,
  `musicClosedCaption`. Za epizodo The Joe Rogan Experience 3764 razdelkov.
- PF `getEpisodeOrChapter`: `previewPlayback.audioPreview.cdnUrl` (mp3).
  Za skladbe Spotify v odgovoru skladbe predogleda ne vrne (`trackPreview`
  zavrne vse preizkušene spremenljivke), zato samo epizode.
- `fetchPlaylist` `revisionId` zamuja za vsebino: po dodajanju dveh skladb je
  odgovor že imel 6 vrstic in še staro revizijo. Osveževanje zato primerja
  pojavitve (uid, URI) prve strani in konec seznama.

## Živi preizkusi v aplikaciji (`sp18`, `sp20`, `sp21`)

```
search profiles -> count 50, items: Spotify, profile // ...
menu on a profile row: Open | Follow  Ctrl+Shift+I | Copy link
profile -> Public playlists, section, 1545 items // Following, section, 327 items // Followers, section, 1000 items; focus ListBox 'Spotify, profile, 12151917 followers'
follow: Following.   unfollow: No longer following.
public playlists -> count 46 ... rows after paging: 86
menu on 'Rock Classics, playlist, Spotify': ... | Go to owner's profile | Copy link
owner profile -> Public playlists ... (profil Spotify)
settings: ... Static:'Sort library by', ComboBox:'Recents', Button:'OK', Button:'Cancel'
library alphabetical -> Liked Songs // #2016, album // #77, album // ...
menu on the test playlist: ... | Rename playlist | Edit description | Make public or private | ...
added two tracks from outside at 02:11:50
after refresh (02:12:02): count 2 ...; focus ListBox 'ApricotPlayer open points test, playlist, Urh.strakl'
dialog: controls 'Static:Description, Edit:, Button:OK, Button:Cancel'
status: Description saved.
dialog again: text 'Made by the ApricotPlayer test' (cancelled)
visibility 1: Private: ApricotPlayer open points test.
visibility 2: Public: ApricotPlayer open points test.
remove from library: Removed from your library.
menu on an episode: Play | Add to Spotify queue | Save to Liked Songs | Add to Spotify playlist | Play preview | Copy link
preview: title 'Preview: #2404 - Elon Musk'; Playing: Preview: #2404 - Elon Musk
transcript window 'Spotify transcript': (Music playing) / Speaker 1: Exactly. / Speaker 2: Just every morning. / ...
```

Apricot prednost (`sp21`): Apricotov seznam s Spotify skladbo "Her Majesty"
(26 s) in lokalno datoteko, Spotify samodejno predvajanje vklopljeno,
Apricotov Autoplay next vklopljen:

```
0 s: title 'Her Majesty - Remastered 2009'; status 'Playing: Her Majesty - Remastered 2009'
27 s: title 'tone'; status 'Playing: tone'
```

Prej (pred SD-13) je isti preizkus končal z "Playback finished.", ker je
Enter na postavki seznama počistil zaporedje; Python v tem primeru nadaljuje
v seznamu (`play_selected_user_playlist_item`, `relative_player_item`).
Z izklopljenim Autoplay next se predvajanje po Spotify skladbi konča in
Spotifyjev samodejni izbor se ne začne.

## Preverjanje

- `cargo clippy --workspace --all-targets -- -D warnings`: brez opozoril.
- `cargo test --workspace`: vsi testi gredo skozi. Novi testi: prepis
  (časi, govorec, glasbeni napis), vrstice profila (playlist z lastnikom,
  profil, izvajalec, neznani URI izpuščen), lastnik playlista v vrstici,
  kontekstni meni (profil, lastni seznam, epizoda), nastavitev razvrščanja,
  postavka Apricotovega seznama nadaljuje v seznamu.
