from apricot.constants import *
import wx
import os
from pathlib import Path

class SearchUI:
    def normalized_podcast_search_provider(self) -> str:
        provider = str(getattr(self.settings, "podcast_search_provider", PODCAST_DIRECTORY_PROVIDER_APPLE) or PODCAST_DIRECTORY_PROVIDER_APPLE)
        return provider if provider in PODCAST_DIRECTORY_PROVIDER_OPTIONS else PODCAST_DIRECTORY_PROVIDER_APPLE

    def normalized_podcast_search_country(self) -> str:
        country = str(getattr(self.settings, "podcast_search_country", "US") or "US").upper()
        return country if country in PODCAST_COUNTRY_OPTIONS else "US"

