import yt_dlp
ydl = yt_dlp.YoutubeDL({'extract_flat': 'in_playlist', 'quiet': True})
info = ydl.extract_info('scsearch5:skrillex', download=False)
for e in info.get('entries', []):
    print(f"Type: {e.get('_type')}, Title: {e.get('title')}, URL: {e.get('url')}")
