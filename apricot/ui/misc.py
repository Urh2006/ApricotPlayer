from apricot.constants import *
import re
import wx
import os
from pathlib import Path

# ---------------------------------------------------------------------------
# Pre-compiled regex patterns — compiling once at module load is measurably
# faster than re-compiling an identical pattern on every call, especially in
# hot paths like strip_html (called for every result description), natural_sort_key
# (called O(n log n) times when sorting a folder), and safe_folder_name.
# ---------------------------------------------------------------------------
_RE_HTML_BR       = re.compile(r"<br\s*/?>", re.IGNORECASE)
_RE_HTML_TAG      = re.compile(r"<[^>]+>")
_RE_INLINE_SPACE  = re.compile(r"[ \t]+")
_RE_NEWLINE_SPACE = re.compile(r"\n\s+")
_RE_WHITESPACE    = re.compile(r"\s+")
_RE_FILE_EXT      = re.compile(r"\.([^./\\]+)$")
_RE_INT_SPLIT     = re.compile(r"(\d+)")
_RE_LRC_LINE      = re.compile(r'^\[(\d+):(\d+\.\d+)\](.*)$')
_RE_UNSAFE_CHARS  = re.compile(r'[<>:"/\\|?*\x00-\x1f]')

# ---------------------------------------------------------------------------
# JAWS COM API — ctypes-only, no external dependencies required.
#
# Talks directly to a running JAWS process via its IDispatch COM automation
# server ("FreedomSci.JawsApi").  A fresh IDispatch reference is acquired on
# every call through CoGetActiveObject so stale-pointer issues cannot occur
# even if JAWS restarts mid-session.
#
# CLSID lookup (CLSIDFromProgID) is performed once and cached; it succeeds
# as soon as JAWS is installed (registry entry present), even when JAWS is
# not currently running.  When JAWS is not installed the result is None and
# all subsequent calls return False immediately with near-zero overhead.
# ---------------------------------------------------------------------------
if os.name == "nt":
    class _JAWS_GUID(ctypes.Structure):
        _fields_ = [
            ("Data1", ctypes.c_uint32),
            ("Data2", ctypes.c_uint16),
            ("Data3", ctypes.c_uint16),
            ("Data4", ctypes.c_uint8 * 8),
        ]

    class _JAWS_VARIANT(ctypes.Structure):
        """Minimal 16-byte VARIANT covering VT_BOOL and VT_BSTR only."""
        class _Val(ctypes.Union):
            _fields_ = [
                ("boolVal", ctypes.c_int16),    # VT_BOOL  (11)
                ("bstrVal", ctypes.c_void_p),   # VT_BSTR  (8)
                ("_pad",    ctypes.c_int64),    # ensures 8-byte slot
            ]
        _fields_ = [
            ("vt",   ctypes.c_uint16), ("_r1", ctypes.c_uint16),
            ("_r2",  ctypes.c_uint16), ("_r3", ctypes.c_uint16),
            ("_val", _Val),
        ]

    class _JAWS_DISPPARAMS(ctypes.Structure):
        _fields_ = [
            ("rgvarg",            ctypes.POINTER(_JAWS_VARIANT)),
            ("rgdispidNamedArgs", ctypes.c_void_p),
            ("cArgs",             ctypes.c_uint32),
            ("cNamedArgs",        ctypes.c_uint32),
        ]

    # IID_IDispatch: {00020400-0000-0000-C000-000000000046}
    _JAWS_IID_IDISPATCH = _JAWS_GUID(
        0x00020400, 0x0000, 0x0000,
        (ctypes.c_uint8 * 8)(0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46),
    )

    _JAWS_CLSID: "type[_JAWS_GUID] | None" = None
    _JAWS_CLSID_READY: bool = False

    def _jaws_get_clsid():
        """Resolve 'FreedomSci.JawsApi' to a CLSID once and cache it.

        Returns a _JAWS_GUID on success (JAWS installed) or None if JAWS is
        not installed / the ProgID is not registered on this machine.
        """
        global _JAWS_CLSID, _JAWS_CLSID_READY
        if _JAWS_CLSID_READY:
            return _JAWS_CLSID
        _JAWS_CLSID_READY = True
        try:
            ole32 = ctypes.windll.ole32
            ole32.CLSIDFromProgID.argtypes = [ctypes.c_wchar_p, ctypes.POINTER(_JAWS_GUID)]
            ole32.CLSIDFromProgID.restype  = ctypes.c_long
            clsid = _JAWS_GUID()
            if ole32.CLSIDFromProgID("FreedomSci.JawsApi", ctypes.byref(clsid)) == 0:
                _JAWS_CLSID = clsid
        except Exception:
            pass
        return _JAWS_CLSID

    def _jaws_speak_ctypes(text: str, flush: bool = True) -> bool:
        """Speak *text* via the JAWS COM IDispatch automation server.

        Uses only ctypes — no external packages (pywin32, comtypes, …) needed.
        Returns True when JAWS was running and the SayString call succeeded.
        Falls back gracefully on any error, including JAWS not being installed
        or not running.

        IDispatch vtable layout (COM standard):
          slot 0  QueryInterface
          slot 1  AddRef
          slot 2  Release
          slot 3  GetTypeInfoCount
          slot 4  GetTypeInfo
          slot 5  GetIDsOfNames
          slot 6  Invoke
        """
        clsid = _jaws_get_clsid()
        if clsid is None:
            return False   # JAWS not installed — skip without any COM calls
        try:
            ole32    = ctypes.windll.ole32
            oleaut32 = ctypes.windll.oleaut32

            # Set up argtypes/restype for the COM helper functions so ctypes
            # handles pointer sizes correctly on both 32-bit and 64-bit Windows.
            ole32.CoInitialize.argtypes    = [ctypes.c_void_p]
            ole32.CoInitialize.restype     = ctypes.c_long
            ole32.CoGetActiveObject.argtypes = [
                ctypes.POINTER(_JAWS_GUID),
                ctypes.POINTER(_JAWS_GUID),
                ctypes.POINTER(ctypes.c_void_p),
            ]
            ole32.CoGetActiveObject.restype  = ctypes.c_long
            oleaut32.SysAllocString.argtypes = [ctypes.c_wchar_p]
            oleaut32.SysAllocString.restype  = ctypes.c_void_p
            oleaut32.SysFreeString.argtypes  = [ctypes.c_void_p]
            oleaut32.SysFreeString.restype   = None

            # Ensure COM is initialised for this thread (no-op if already done).
            ole32.CoInitialize(None)

            # Ask Windows for a reference to the already-running JAWS IDispatch.
            pdisp = ctypes.c_void_p()
            hr = ole32.CoGetActiveObject(
                ctypes.byref(clsid),
                ctypes.byref(_JAWS_IID_IDISPATCH),
                ctypes.byref(pdisp),
            )
            if hr != 0 or not pdisp.value:
                return False   # JAWS not currently running

            disp   = pdisp.value                   # raw IDispatch* as Python int
            ptr_sz = ctypes.sizeof(ctypes.c_size_t)

            # The IDispatch interface pointer points to a struct whose first
            # member is a pointer to the vtable (array of function pointers).
            vtbl = ctypes.c_size_t.from_address(disp).value

            def _vtfn_addr(slot: int) -> int:
                return ctypes.c_size_t.from_address(vtbl + slot * ptr_sz).value

            try:
                # ── vtable slot 5: GetIDsOfNames ─────────────────────────
                GetIDsOfNames = ctypes.WINFUNCTYPE(
                    ctypes.c_long,                     # HRESULT
                    ctypes.c_void_p,                   # this
                    ctypes.c_void_p,                   # riid  (IID_NULL → None)
                    ctypes.POINTER(ctypes.c_wchar_p),  # rgszNames
                    ctypes.c_uint32,                   # cNames
                    ctypes.c_uint32,                   # lcid
                    ctypes.POINTER(ctypes.c_long),     # rgDispId
                )(_vtfn_addr(5))

                names  = (ctypes.c_wchar_p * 1)("SayString")
                dispid = ctypes.c_long(-1)
                hr = GetIDsOfNames(
                    ctypes.c_void_p(disp), None,
                    names, 1, 0x0400,               # 0x0400 = LOCALE_USER_DEFAULT
                    ctypes.byref(dispid),
                )
                if hr != 0 or dispid.value < 0:
                    return False

                # ── Build DISPPARAMS ─────────────────────────────────────
                # COM IDispatch passes arguments in reverse declaration order.
                # SayString(text: BSTR, flush: BOOL)
                #   argv[0] = flush (last declared param, first in rgvarg)
                #   argv[1] = text  (first declared param, last in rgvarg)
                argv = (_JAWS_VARIANT * 2)()

                argv[0].vt          = 11                      # VT_BOOL
                argv[0]._val.boolVal = -1 if flush else 0     # VARIANT_BOOL: -1=True

                bstr = oleaut32.SysAllocString(text)
                if not bstr:
                    return False
                argv[1].vt           = 8                      # VT_BSTR
                argv[1]._val.bstrVal = bstr

                dp            = _JAWS_DISPPARAMS()
                dp.rgvarg     = argv
                dp.cArgs      = 2
                dp.cNamedArgs = 0

                # ── vtable slot 6: Invoke ────────────────────────────────
                Invoke = ctypes.WINFUNCTYPE(
                    ctypes.c_long,                         # HRESULT
                    ctypes.c_void_p,                       # this
                    ctypes.c_long,                         # dispIdMember
                    ctypes.c_void_p,                       # riid  (IID_NULL)
                    ctypes.c_uint32,                       # lcid
                    ctypes.c_uint16,                       # wFlags
                    ctypes.POINTER(_JAWS_DISPPARAMS),      # pDispParams
                    ctypes.c_void_p,                       # pVarResult  (unused)
                    ctypes.c_void_p,                       # pExcepInfo  (unused)
                    ctypes.c_void_p,                       # puArgErr    (unused)
                )(_vtfn_addr(6))

                hr = Invoke(
                    ctypes.c_void_p(disp),
                    dispid.value,
                    None,          # IID_NULL
                    0x0400,        # LOCALE_USER_DEFAULT
                    1,             # DISPATCH_METHOD
                    ctypes.byref(dp),
                    None, None, None,
                )
                oleaut32.SysFreeString(bstr)
                return hr == 0

            finally:
                # Always release the IDispatch reference (vtable slot 2).
                Release = ctypes.WINFUNCTYPE(
                    ctypes.c_ulong, ctypes.c_void_p,
                )(_vtfn_addr(2))
                Release(ctypes.c_void_p(disp))

        except Exception:
            return False

else:
    def _jaws_speak_ctypes(text: str, flush: bool = True) -> bool:  # type: ignore[misc]
        """No-op on non-Windows platforms."""
        return False


class ApricotTaskBarIcon(wx.adv.TaskBarIcon):
    def __init__(self, frame: "MiscUI") -> None:
        super().__init__()
        self.frame = frame
        self.show_id = wx.NewIdRef()
        self.settings_id = wx.NewIdRef()
        self.check_id = wx.NewIdRef()
        self.exit_id = wx.NewIdRef()
        for event_name in ("EVT_TASKBAR_LEFT_UP", "EVT_TASKBAR_LEFT_DCLICK"):
            event_binder = getattr(wx.adv, event_name, None)
            if event_binder is not None:
                self.Bind(event_binder, lambda _event: self.frame.restore_from_tray())
        self.Bind(wx.EVT_MENU, lambda _event: self.frame.restore_from_tray(), id=int(self.show_id))
        self.Bind(wx.EVT_MENU, lambda _event: self.frame.show_settings_from_tray(), id=int(self.settings_id))
        self.Bind(wx.EVT_MENU, lambda _event: self.frame.check_subscriptions(manual=True), id=int(self.check_id))
        self.Bind(wx.EVT_MENU, lambda _event: self.frame.quit_application(), id=int(self.exit_id))
        self.SetIcon(self.make_icon(), APP_NAME)

    def CreatePopupMenu(self) -> wx.Menu:
        menu = wx.Menu()
        menu.Append(int(self.show_id), self.frame.t("tray_show"))
        menu.Append(int(self.settings_id), self.frame.t("tray_settings"))
        menu.Append(int(self.check_id), self.frame.t("tray_check_subscriptions"))
        menu.AppendSeparator()
        menu.Append(int(self.exit_id), self.frame.t("tray_exit"))
        return menu

    @staticmethod
    def make_icon() -> wx.Icon:
        bitmap = wx.ArtProvider.GetBitmap(wx.ART_INFORMATION, wx.ART_OTHER, (16, 16))
        icon = wx.Icon()
        icon.CopyFromBitmap(bitmap)
        return icon


class MiscUI:
    @staticmethod
    def focus_accepts_text(focus: wx.Window | None) -> bool:
        if focus is None:
            return False
        try:
            return isinstance(focus, wx.TextCtrl) and not bool(focus.GetWindowStyleFlag() & wx.TE_READONLY)
        except Exception:
            return False

    def t(self, key: str, **kwargs) -> str:
        language = self.settings.language if self.settings.language in TEXT else "en"
        text = TEXT[language].get(key, TEXT["en"].get(key, key))
        return text.format(**kwargs) if kwargs else text

    @staticmethod
    def ytdlp_ejs_available() -> bool:
        try:
            import_module("yt_dlp_ejs")
            return True
        except Exception:
            return False

    def bundled_node_executable(self) -> str:
        candidates = [
            self.bundled_path("node", "node.exe"),
            Path(__file__).resolve().parent / "vendor" / "node" / "node.exe",
        ]
        for candidate in candidates:
            if candidate.exists():
                return str(candidate)
        node = shutil.which("node")
        return node or ""

    def ytdlp_js_runtimes(self) -> dict:
        node = self.bundled_node_executable()
        if node:
            return {"node": {"path": node}}
        return {"deno": {}, "node": {}, "quickjs": {}, "bun": {}}

    def friendly_error(self, exc: Exception | str, include_youtube_auth_hint: bool = True) -> str:
        text = str(exc)
        lowered = text.lower()
        if "failed to decrypt with dpapi" in lowered or "nonetype" in lowered and "decode" in lowered:
            return f"{text}\n\n{self.t('cookie_copy_hint')}"
        if "could not copy" in lowered and "cookie" in lowered and "database" in lowered:
            return f"{text}\n\n{self.t('cookie_copy_hint')}"
        if include_youtube_auth_hint and ("sign in to confirm" in lowered or "not a bot" in lowered or "cookies-from-browser" in lowered):
            return f"{text}\n\n{self.t('youtube_auth_hint')}"
        return text

    @staticmethod
    def free_local_port() -> int:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.bind(("127.0.0.1", 0))
            return int(sock.getsockname()[1])

    def set_window_title(self, media_title: str | None = None) -> None:
        title = _RE_WHITESPACE.sub(" ", str(media_title or "").strip())
        if title:
            window_title = f"{title} - {WINDOW_TITLE}"
        else:
            window_title = WINDOW_TITLE
        try:
            if self.GetTitle() == window_title:
                return
        except Exception:
            pass
        self.SetTitle(window_title)

    def current_play_pause_label(self) -> str:
        if not self.player_is_active() or self.player_ended or self.player_paused:
            return self.t("play")
        return self.t("pause")

    def clear(self) -> None:
        self.podcast_categories_screen_active = False
        preserved_player_panel = None
        if self.player_is_active() and self.player_panel is not None:
            try:
                if not self.player_panel.IsBeingDeleted():
                    preserved_player_panel = self.player_panel
                    self.root_sizer.Detach(preserved_player_panel)
                    preserved_player_panel.Hide()
            except RuntimeError:
                preserved_player_panel = None
        if self.player_is_active():
            self.set_window_title(self.current_player_title())
        elif not self.in_player_screen:
            self.set_window_title()
        self.root_sizer.Clear(delete_windows=True)
        self.background_player_controls = []
        self.player_action_controls = []
        self.player_play_pause_buttons = []
        self.background_player_previous_control = None
        self.last_button_row_controls = []
        self.background_player_section_added = False
        self.background_player_section_pending = False
        self.background_player_section_generation += 1
        self.settings_screen_active = False
        if not self.in_player_screen:
            if preserved_player_panel is not None:
                self.player_panel = preserved_player_panel
            elif not self.player_is_active():
                self.player_panel = None
            self.details_label = None
            self.video_details = None
            self.details_button_sizer = None
            self.details_opened_temporarily = False

    def focus_later(self, control: wx.Window) -> None:
        wx.CallAfter(self.safe_set_focus, control)

    @staticmethod
    def safe_set_focus(control: wx.Window) -> None:
        try:
            if control and not getattr(control, "IsBeingDeleted", lambda: False)():
                if wx.Window.FindFocus() is control:
                    return
                control.SetFocus()
        except RuntimeError:
            pass

    def set_integer_slider_accessibility(self, ctrl: wx.Slider, label: str, unit: str = "") -> None:
        value = int(ctrl.GetValue())
        name = str(label).strip()
        value_text = self.t("download_percent_value", percent=value) if unit == "percent" else f"{value} {unit}".strip()
        full_text = f"{name}: {value_text}" if value_text else name
        ctrl.SetName(full_text)
        ctrl.SetLabel(full_text)
        ctrl.SetToolTip(full_text)
        ctrl._apricot_accessible_name = name
        ctrl._apricot_accessible_description = full_text
        ctrl._apricot_accessible_value = value_text
        if not getattr(ctrl, "_apricot_accessible", None):
            try:
                ctrl._apricot_accessible = SliderAccessible(ctrl)
                ctrl.SetAccessible(ctrl._apricot_accessible)
            except Exception:
                pass
        try:
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_NAMECHANGE, ctrl, wx.OBJID_CLIENT, 0)
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_VALUECHANGE, ctrl, wx.OBJID_CLIENT, 0)
        except Exception:
            pass

    def foreground_window(self) -> None:
        try:
            self.Show(True)
        except Exception:
            pass
        try:
            if self.IsIconized():
                self.Iconize(False)
        except Exception:
            pass
        try:
            self.Raise()
        except Exception:
            pass
        if os.name == "nt":
            try:
                hwnd = int(self.GetHandle())
                if hwnd:
                    user32 = ctypes.windll.user32
                    kernel32 = ctypes.windll.kernel32
                    foreground = user32.GetForegroundWindow()
                    foreground_thread = user32.GetWindowThreadProcessId(foreground, None) if foreground else 0
                    target_thread = user32.GetWindowThreadProcessId(hwnd, None)
                    current_thread = kernel32.GetCurrentThreadId()
                    attached: list[tuple[int, int]] = []
                    for source_thread, target_input_thread in (
                        (current_thread, foreground_thread),
                        (target_thread, foreground_thread),
                    ):
                        if source_thread and target_input_thread and source_thread != target_input_thread:
                            try:
                                if user32.AttachThreadInput(source_thread, target_input_thread, True):
                                    attached.append((source_thread, target_input_thread))
                            except Exception:
                                pass
                    try:
                        user32.ShowWindow(hwnd, 9)
                        user32.BringWindowToTop(hwnd)
                        user32.SetForegroundWindow(hwnd)
                        user32.SetActiveWindow(hwnd)
                        user32.SetFocus(hwnd)
                    finally:
                        for source_thread, target_input_thread in reversed(attached):
                            try:
                                user32.AttachThreadInput(source_thread, target_input_thread, False)
                            except Exception:
                                pass
                    try:
                        if user32.GetForegroundWindow() != hwnd:
                            self.RequestUserAttention(wx.USER_ATTENTION_INFO)
                    except Exception:
                        pass
            except Exception:
                pass

    def primary_focus_candidate(self) -> wx.Window | None:
        if getattr(self, "in_main_menu", False) and hasattr(self, "menu_list"):
            return self.menu_list
        if getattr(self, "search_screen_active", False) and hasattr(self, "query"):
            return self.query
        focus = wx.Window.FindFocus()
        return focus or self

    def focus_primary_control(self) -> None:
        focus = self.primary_focus_candidate()
        if focus:
            self.safe_set_focus(focus)

    def ensure_window_visible(self) -> None:
        try:
            if not self.IsShown():
                self.Show()
            if self.IsIconized():
                self.Iconize(False)
            self.Raise()
        except RuntimeError:
            pass

    def activate_window(self) -> None:
        self.ensure_window_visible()
        focus = wx.Window.FindFocus()
        primary = self.primary_focus_candidate()
        if primary is not None and focus is primary and self.app_has_focus():
            return
        self.foreground_window()
        self.focus_primary_control()

    def activate_window_later(self, delays: tuple[int, ...] = (0, 250)) -> None:
        for delay in delays:
            if delay <= 0:
                wx.CallAfter(self.activate_window)
            else:
                wx.CallLater(delay, self.activate_window)

    def speak_text(self, text: str) -> None:
        if not text:
            return
        announced = False

        # ── NVDA controller ───────────────────────────────────────────────────
        client = self.ensure_nvda_client()
        if client:
            try:
                if hasattr(client, "nvdaController_cancelSpeech"):
                    client.nvdaController_cancelSpeech()
                result = client.nvdaController_speakText(str(text))
                if result == 0:
                    announced = True
                if hasattr(client, "nvdaController_brailleMessage"):
                    braille_result = client.nvdaController_brailleMessage(str(text))
                    if braille_result == 0:
                        announced = True
            except Exception:
                self.nvda_client = None

        # ── JAWS COM API ──────────────────────────────────────────────────────
        # Only attempted when NVDA did not handle the text — avoids duplicate
        # speech on the rare system where both screen readers run simultaneously.
        if not announced:
            try:
                if _jaws_speak_ctypes(str(text)):
                    announced = True
            except Exception:
                pass

        # ── WinEvents (Narrator / other IAccessible-based screen readers) ─────
        # EVENT_OBJECT_NAMECHANGE and EVENT_OBJECT_VALUECHANGE are always fired
        # so that Narrator and any other IAccessible SR still receives the text.
        #
        # EVENT_SYSTEM_ALERT is suppressed when NVDA or JAWS already received
        # the text directly:
        #   • NVDA monitors EVENT_SYSTEM_ALERT independently of its controller
        #     API.  Firing it after nvdaController_speakText causes NVDA to read
        #     the text a second time, interrupting itself and restarting the
        #     Windows audio-ducking timer — making the system speakers appear
        #     permanently muted while the player window has focus.
        #   • JAWS may also respond to EVENT_SYSTEM_ALERT independently; skipping
        #     it when SayString succeeded prevents a potential double-announce.
        self.raise_accessibility_alert(text, skip_alert_event=announced)
        if announced:
            return

    def raise_accessibility_alert(self, text: str, skip_alert_event: bool = False) -> None:
        self.SetName(text)
        try:
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_NAMECHANGE, self, wx.OBJID_CLIENT, 0)
            if not skip_alert_event:
                # EVENT_SYSTEM_ALERT causes NVDA to re-read the text.  Suppress it
                # when NVDA already received the text via nvdaController_speakText
                # to prevent double-announcement and the resulting audio ducking.
                wx.Accessible.NotifyEvent(wx.ACC_EVENT_SYSTEM_ALERT, self, wx.OBJID_ALERT, 0)
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_VALUECHANGE, self.status, wx.OBJID_CLIENT, 0)
        except Exception:
            pass

    def ensure_nvda_client(self):
        if not getattr(self, "nvda_client_load_attempted", False):
            self.nvda_client_load_attempted = True
            self.nvda_client = self.load_nvda_client()
        return self.nvda_client

    def load_nvda_client(self):
        for path in self.nvda_client_candidates():
            try:
                if path.exists():
                    client = ctypes.WinDLL(str(path))
                    client.nvdaController_speakText.argtypes = [ctypes.c_wchar_p]
                    client.nvdaController_speakText.restype = ctypes.c_int
                    if hasattr(client, "nvdaController_brailleMessage"):
                        client.nvdaController_brailleMessage.argtypes = [ctypes.c_wchar_p]
                        client.nvdaController_brailleMessage.restype = ctypes.c_int
                    if hasattr(client, "nvdaController_cancelSpeech"):
                        client.nvdaController_cancelSpeech.argtypes = []
                        client.nvdaController_cancelSpeech.restype = ctypes.c_int
                    return client
            except Exception:
                continue
        return None

    def nvda_client_candidates(self) -> list[Path]:
        names = ["nvdaControllerClient64.dll", "nvdaControllerClient.dll"]
        roots = [
            self.bundled_path("nvda"),
            Path(__file__).resolve().parent / "vendor" / "nvda",
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "NVDA",
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Bookworm" / "accessible_output2" / "lib",
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "TeamTalk5",
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "twblue" / "accessible_output2" / "lib",
            Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "RS Games Client" / "accessible_output2" / "lib",
        ]
        candidates = []
        for root in roots:
            for name in names:
                candidates.append(root / name)
        return candidates

    def setup_taskbar_icon(self) -> None:
        if getattr(self, "exiting", False):
            return
        if self.taskbar_icon is not None:
            return
        try:
            self.taskbar_icon = ApricotTaskBarIcon(self)
        except Exception:
            self.taskbar_icon = None

    def destroy_taskbar_icon(self) -> None:
        if self.taskbar_icon is None:
            return
        try:
            self.taskbar_icon.RemoveIcon()
            self.taskbar_icon.Destroy()
        except Exception:
            pass
        self.taskbar_icon = None

    @staticmethod
    def windows_startup_value_name() -> str:
        return APP_NAME

    @staticmethod
    def current_launch_command(start_in_tray: bool = False) -> str:
        if getattr(sys, "frozen", False):
            parts = [sys.executable]
        else:
            parts = [sys.executable, str(Path(__file__).resolve())]
        if start_in_tray:
            parts.append(START_IN_TRAY_ARG)
        return subprocess.list2cmdline(parts)

    def sync_windows_startup_registration(self, show_error: bool = False) -> bool:
        if os.name != "nt" or winreg is None:
            return False
        try:
            access = winreg.KEY_SET_VALUE | winreg.KEY_QUERY_VALUE
            with winreg.CreateKeyEx(winreg.HKEY_CURRENT_USER, self.windows_startup_run_key_path(), 0, access) as key:
                value_name = self.windows_startup_value_name()
                if self.settings.start_with_windows:
                    command = self.current_launch_command(start_in_tray=False)
                    current = ""
                    try:
                        current, _value_type = winreg.QueryValueEx(key, value_name)
                    except FileNotFoundError:
                        current = ""
                    if str(current) != command:
                        winreg.SetValueEx(key, value_name, 0, winreg.REG_SZ, command)
                else:
                    try:
                        winreg.DeleteValue(key, value_name)
                    except FileNotFoundError:
                        pass
            return True
        except Exception as exc:
            if show_error:
                self.message(self.t("startup_registration_failed", error=self.friendly_error(exc)), wx.ICON_WARNING)
            return False

    def restore_from_tray(self) -> None:
        self.ensure_window_visible()
        try:
            self.RequestUserAttention(wx.USER_ATTENTION_INFO)
        except Exception:
            pass
        try:
            self.activate_window()
        except Exception:
            pass
        self.activate_window_later((0, 75, 250, 700, 1400))

    def show_settings_from_tray(self) -> None:
        self.restore_from_tray()
        wx.CallAfter(self.open_settings_screen)

    def quit_application(self) -> None:
        self.shutdown_runtime()
        self.Close(force=True)

    def shutdown_runtime(self) -> None:
        self.exiting = True
        for timer in (
            getattr(self, "timer", None),
            getattr(self, "subscription_timer", None),
            getattr(self, "rss_timer", None),
            getattr(self, "app_update_timer", None),
        ):
            try:
                if timer and timer.IsRunning():
                    timer.Stop()
            except Exception:
                pass
        try:
            self.save_stream_url_cache()
        except Exception:
            pass
        try:
            self.save_history()
        except Exception:
            pass
        try:
            self.stop_player(silent=True)
        except Exception:
            pass
        self.destroy_taskbar_icon()

    def app_has_focus(self) -> bool:
        try:
            if self.IsShown() and self.IsActive():
                return True
        except Exception:
            pass
        try:
            focus = wx.Window.FindFocus()
            if focus and focus.GetTopLevelParent() is self:
                return True
        except Exception:
            pass
        return False

    def show_desktop_notification(
        self,
        title: str,
        message: str,
        enabled: bool = True,
        only_when_unfocused: bool = False,
    ) -> bool:
        if not enabled or not self.settings.windows_notifications:
            return False
        if only_when_unfocused and self.app_has_focus():
            return False
        try:
            notification = wx.adv.NotificationMessage(title=title, message=message, parent=self)
            notification.Show(timeout=10)
            return True
        except Exception:
            return False

    @staticmethod
    def live_window(control: wx.Window | None) -> wx.Window | None:
        if control is None:
            return None
        try:
            if control.IsBeingDeleted():
                return None
            if not control.IsShownOnScreen() and not control.IsShown():
                return None
        except RuntimeError:
            return None
        except Exception:
            pass
        return control

    @staticmethod
    def window_is_or_descendant(window: wx.Window | None, ancestor: wx.Window | None) -> bool:
        if window is None or ancestor is None:
            return False
        current = window
        while current is not None:
            if current is ancestor:
                return True
            try:
                current = current.GetParent()
            except RuntimeError:
                return False
            except Exception:
                return False
        return False

    def exit_fullscreen_window(self) -> None:
        self.player_fullscreen_session = False
        self.player_fullscreen_results_override = True
        try:
            if self.player_kind == "mpv" and self.mpv_process_alive():
                self.mpv_request(["set_property", "fullscreen", False], timeout=0.5)
        except Exception:
            pass
        try:
            if self.IsFullScreen():
                self.ShowFullScreen(False)
        except Exception:
            pass

    def start_folder_conversion(self, source_folder: Path, output_folder: Path, target_format: str, image_path: Path | None = None, replace_originals: bool = False) -> None:
        self.announce_player(self.t("conversion_started"))
        self.set_status(self.t("conversion_started"))
        threading.Thread(target=self.folder_conversion_worker, args=(source_folder, output_folder, target_format, image_path, replace_originals), daemon=True).start()

    def run_ffmpeg_conversion(self, args: list[str]) -> None:
        creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        result = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=creationflags)
        if result.returncode != 0:
            error = (result.stderr or result.stdout or "").strip() or f"FFmpeg exited with code {result.returncode}"
            raise RuntimeError(error[-900:])

    def clear_notifications(self) -> None:
        self.notifications = []
        self.save_notifications()
        self.refresh_notification_center()
        self.announce_player(self.t("notifications_cleared"))

    def add_app_notification(self, notification: dict) -> None:
        item = notification.get("item") if isinstance(notification.get("item"), dict) else {}
        stored = {
            "kind": notification.get("kind", "info"),
            "title": notification.get("title", APP_NAME),
            "message": notification.get("message", ""),
            "item": item,
            "timestamp": time.time(),
        }
        self.notifications.insert(0, stored)
        self.notifications = self.notifications[:200]
        self.save_notifications()
        if self.notification_center_screen_active:
            self.refresh_notification_center()
        if not self.app_has_focus():
            enabled = self.settings.windows_notifications
            if stored.get("kind") == "subscription":
                enabled = enabled and self.settings.subscription_notifications
            self.show_desktop_notification(str(stored.get("title") or APP_NAME), str(stored.get("message") or ""), enabled=enabled)

    def queue_line(self, item: dict) -> str:
        if item.get("queue_state") == "active":
            state = self.t(str(item.get("status_key") or "download_state_downloading"))
            kind = str(item.get("task_kind") or "")
            if kind == "batch":
                completed = int(item.get("completed") or 0)
                total = int(item.get("total") or 0)
                remaining = max(0, total - completed)
                summary = self.t("downloads_remaining", remaining=remaining, total=total) if total else ""
            else:
                total = int(item.get("playlist_count") or 0)
                index = int(item.get("playlist_index") or 0)
                remaining = max(0, total - index) if total and index else 0
                summary = self.t("downloads_remaining", remaining=remaining, total=total) if total and index else ""
            current = item.get("current_title") or item.get("title", "")
            percent = item.get("percent")
            percent_text = self.t("download_percent_value", percent=percent) if percent else ""
            parts = [item.get("title", ""), state, summary, current, percent_text]
            return " | ".join(part for part in parts if part)
        mode = self.queue_mode_label(item)
        parts = [
            item.get("title", ""),
            item.get("type", ""),
            f"{self.t('channel')}: {item.get('channel', '')}" if item.get("channel") and item.get("kind") == "video" else "",
            mode,
            self.t("download_state_queued"),
        ]
        return " | ".join(part for part in parts if part)

    def queue_mode_label(self, item: dict) -> str:
        if item.get("kind") == "rss_item":
            return self.t("podcast_audio_queued_marker")
        if not isinstance(item.get("audio_only"), bool):
            return self.t("selected_queued_marker")
        if item.get("kind") in {"playlist", "channel"}:
            if item.get("audio_only"):
                return self.t("collection_audio_queued_marker")
            return self.t("collection_video_queued_marker")
        return self.t("audio_queued_marker" if item.get("audio_only") else "video_queued_marker")

    def refresh_interval_seconds(self, value, default: float, maximum_hours: float = 168.0) -> float:
        hours = self.to_float(str(value), default, 0.5, maximum_hours)
        return max(30 * 60, hours * 60 * 60)

    def import_opml_worker(self, to_import: list[tuple[str, str]]) -> None:
        imported_count = 0
        failed_count = 0
        
        for i, (url, title) in enumerate(to_import):
            self.ui_queue.put(("announce", self.t("opml_import_progress", current=i + 1, total=len(to_import), title=title)))
            try:
                feed = self.fetch_rss_feed(url)
                self.rss_feeds.append(feed)
                imported_count += 1
            except Exception:
                failed_count += 1
                
        if imported_count > 0:
            self.save_rss_feeds()
            self.ui_queue.put(("rss_feeds_changed", None))
            
        if failed_count == 0:
            self.ui_queue.put(("announce", self.t("opml_import_done", count=imported_count)))
        else:
            self.ui_queue.put(("announce", self.t("opml_import_done_with_errors", imported=imported_count, failed=failed_count)))

    def parse_feed_root(self, root: ET.Element, base_url: str) -> tuple[str, str, list[dict]]:
        root_name = self.xml_local_name(root.tag)
        if root_name == "feed":
            return self.parse_atom_feed(root, base_url)
        channel = self.first_child(root, "channel") or root
        title = self.child_text(channel, "title")
        site_url = self.absolute_url(self.child_text(channel, "link"), base_url)
        items = [self.parse_rss_item(item, base_url, title) for item in self.children(channel, "item")]
        return title, site_url, [item for item in items if item.get("title") or item.get("url")]

    def parse_atom_feed(self, root: ET.Element, base_url: str) -> tuple[str, str, list[dict]]:
        title = self.child_text(root, "title")
        site_url = self.atom_link(root, base_url, {"alternate", ""})
        items = [self.parse_atom_item(entry, base_url, title) for entry in self.children(root, "entry")]
        return title, site_url, [item for item in items if item.get("title") or item.get("url")]

    def atom_link(self, element: ET.Element, base_url: str, rels: set[str]) -> str:
        for child in self.children(element, "link"):
            rel = str(child.get("rel") or "").lower()
            if rel in rels:
                href = str(child.get("href") or "").strip()
                if href:
                    return self.absolute_url(href, base_url)
        return ""

    @staticmethod
    def xml_local_name(tag: str) -> str:
        return str(tag).split("}", 1)[-1].lower()

    def children(self, element: ET.Element, local_name: str) -> list[ET.Element]:
        return [child for child in list(element) if self.xml_local_name(child.tag) == local_name.lower()]

    def first_child(self, element: ET.Element, local_name: str) -> ET.Element | None:
        for child in self.children(element, local_name):
            return child
        return None

    def child_text(self, element: ET.Element, local_name: str) -> str:
        child = self.first_child(element, local_name)
        if child is None:
            return ""
        return "".join(child.itertext()).strip()

    @staticmethod
    def parse_feed_timestamp(value: str) -> float:
        value = str(value or "").strip()
        if not value:
            return 0.0
        try:
            return parsedate_to_datetime(value).timestamp()
        except Exception:
            pass
        try:
            return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
        except Exception:
            return 0.0

    @staticmethod
    def strip_html(value: str) -> str:
        text = _RE_HTML_BR.sub("\n", str(value or ""))
        text = _RE_HTML_TAG.sub(" ", text)
        text = _RE_INLINE_SPACE.sub(" ", text)
        text = _RE_NEWLINE_SPACE.sub("\n", text)
        return text.strip()

    def show_settings(self) -> None:
        self.last_activated_menu_action = self.show_settings
        self.in_main_menu = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.settings_screen_active = True
        self.add_button_row([(self.t("back"), self.back_from_settings), (self.t("save"), self.save_settings_from_ui), (self.t("restore_defaults"), self.restore_default_settings)])
        self.controls = {}
        self.choice_values = {}
        self.settings_control_order = []
        self.settings_render_generation = 0
        self.settings_pending_section_index = -1
        self.settings_controls_applied_for_pending = False
        sections = self.settings_sections()
        self.settings_section_index = min(max(0, self.settings_section_index), len(sections) - 1)
        body = wx.BoxSizer(wx.HORIZONTAL)
        self.settings_section_list = wx.ListBox(self.panel, choices=[label for label, _name in sections], style=wx.LB_SINGLE)
        self.settings_section_list.SetName(self.t("settings_sections"))
        self.settings_section_list.SetSelection(self.settings_section_index)
        self.settings_section_list.Bind(wx.EVT_LISTBOX, self.on_settings_section_changed)
        self.settings_section_list.Bind(wx.EVT_KEY_DOWN, self.on_settings_section_key)
        body.Add(self.settings_section_list, 0, wx.EXPAND | wx.ALL, 4)
        self.settings_scroller = wx.ScrolledWindow(self.panel, style=wx.VSCROLL | wx.WANTS_CHARS)
        self.settings_scroller.SetName(self.t("settings"))
        self.settings_scroller.SetScrollRate(10, 10)
        body.Add(self.settings_scroller, 1, wx.EXPAND | wx.ALL, 4)
        self.root_sizer.Add(body, 1, wx.EXPAND)
        self.render_settings_section()
        self.panel.Layout()
        self.focus_settings_section_list_later()

    def set_status_if_current(self, generation: int, text: str) -> None:
        if generation == self.search_generation:
            self.set_status(text)

    @staticmethod
    def metadata_live_status(info: dict | None) -> str:
        if not isinstance(info, dict):
            return ""
        snippet = info.get("snippet") if isinstance(info.get("snippet"), dict) else {}
        value = info.get("live_status") or info.get("liveBroadcastContent") or snippet.get("liveBroadcastContent") or ""
        return str(value or "").strip().lower().replace("-", "_")

    @staticmethod
    def metadata_bool(value) -> bool:
        if isinstance(value, str):
            return value.strip().lower() in {"1", "true", "yes", "y", "live", "is_live"}
        return bool(value)

    @classmethod
    def metadata_is_live_stream(cls, info: dict | None) -> bool:
        if not isinstance(info, dict):
            return False
        return MiscUI.metadata_live_status(info) in {"is_live", "live"} or MiscUI.metadata_bool(info.get("is_live"))

    def with_live_stream_display_fields(self, item: dict, source: dict | None = None) -> dict:
        source = source if isinstance(source, dict) else item
        live_status = self.metadata_live_status(source) or self.metadata_live_status(item)
        is_live = self.metadata_is_live_stream(source) or self.metadata_is_live_stream(item)
        if live_status:
            item["live_status"] = live_status
        item["is_live"] = bool(is_live)
        if str(item.get("kind") or "video") == "video" and is_live:
            item["type"] = self.t("live_stream")
            item["age"] = self.t("live_now")
        elif str(item.get("kind") or "video") == "video":
            item["type"] = item.get("type") or self.t("video")
        return item

    def metadata_from_info(self, info: dict, item: dict) -> dict:
        timestamp = info.get("timestamp") or info.get("release_timestamp") or info.get("modified_timestamp") or item.get("timestamp")
        upload_date = info.get("upload_date") or item.get("upload_date")
        is_live = self.metadata_is_live_stream(info) or self.metadata_is_live_stream(item)
        payload = {
            "url": item.get("url", ""),
            "title": info.get("title") or item.get("title", ""),
            "id": info.get("id") or item.get("id", ""),
            "channel": info.get("uploader") or info.get("channel") or item.get("channel", ""),
            "channel_url": self.normalize_channel_url(info) or item.get("channel_url", ""),
            "channel_id": info.get("channel_id") or info.get("uploader_id") or item.get("channel_id", ""),
            "view_count": info.get("view_count", item.get("view_count")),
            "views": self.format_count(info.get("view_count", item.get("view_count"))),
            "timestamp": timestamp,
            "upload_date": upload_date,
            "age": self.t("live_now") if is_live else (self.format_age({"timestamp": timestamp, "upload_date": upload_date}) or item.get("age") or self.t("uploaded_unknown")),
            "duration_seconds": info.get("duration", item.get("duration_seconds")),
            "duration": self.format_duration(info.get("duration", item.get("duration_seconds"))),
            "description": info.get("description") or item.get("description", ""),
            "artist": info.get("artist") or info.get("creator") or item.get("artist", ""),
            "track": info.get("track") or item.get("track", ""),
            "album": info.get("album") or item.get("album", ""),
            "chapters": self.normalized_chapters(info.get("chapters")) or item.get("chapters", []),
            "kind": item.get("kind", "video"),
            "type": self.t("live_stream") if is_live else item.get("type", self.t("video")),
            "live_status": self.metadata_live_status(info) or self.metadata_live_status(item),
            "is_live": is_live,
        }
        return self.with_live_stream_display_fields(payload, info)

    @staticmethod
    def numeric_view_count(value) -> int:
        if value in (None, ""):
            return -1
        try:
            return int(float(str(value).replace(",", "").strip()))
        except (TypeError, ValueError):
            return -1

    def resolve_channel_id_for_popular(self, url: str) -> str:
        existing = str(getattr(self, "collection_channel_id", "") or "").strip()
        if self.is_youtube_channel_id(existing):
            return existing
        try:
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": 1}
            info = self.ydl_extract_info(url, options, download=False, allow_cookie_retry=False)
        except Exception:
            return ""
        channel_id = str((info or {}).get("channel_id") or (info or {}).get("id") or "").strip()
        return channel_id if self.is_youtube_channel_id(channel_id) else ""

    def show_channel_options(self, item: dict | None = None) -> None:
        item = item or self.selected_result()
        if not item or item.get("kind") != "channel":
            self.message(self.t("no_selection"))
            return
        tabs = [
            ("videos", self.t("channel_videos")),
            ("playlists", self.t("channel_playlists")),
            ("streams", self.t("channel_live_streams")),
            ("popular", self.t("channel_popular")),
        ]
        with wx.SingleChoiceDialog(self, item.get("title", self.t("channel")), self.t("channel_options"), [label for _tab, label in tabs]) as dialog:
            dialog.SetSelection(0)
            if dialog.ShowModal() != wx.ID_OK:
                return
            index = dialog.GetSelection()
        if 0 <= index < len(tabs):
            self.open_channel_tab(item, tabs[index][0], push_state=True)

    def show_trending(self, auto_load: bool = True, country_index: int | None = None, category_index: int | None = None) -> None:
        if not getattr(self.settings, "enable_trending", False):
            self.announce_player(self.t("trending_disabled"))
            self.show_main_menu()
            return
        self.last_activated_menu_action = self.show_trending
        self.in_player_screen = False
        if not self.player_is_active():
            self.player_control_mode = False
        self.in_main_menu = False
        self.search_screen_active = True
        self.trending_screen_active = True
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
        self.add_button_row([(self.t("back"), self.show_main_menu), (self.t("load_trending"), self.load_trending_results)])
        grid = wx.FlexGridSizer(2, 2, 6, 6)
        grid.AddGrowableCol(1, 1)
        grid.Add(wx.StaticText(self.panel, label=self.t("trending_country")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.trending_country_choice = wx.Choice(self.panel, choices=[label for _code, label in TRENDING_COUNTRIES])
        self.trending_country_choice.SetName(self.t("trending_country"))
        selected_country = self.last_trending_country_index if country_index is None else country_index
        self.trending_country_choice.SetSelection(min(max(0, selected_country), self.trending_country_choice.GetCount() - 1))
        self.trending_country_choice.Bind(wx.EVT_CHOICE, lambda _evt: self.load_trending_results())
        self.trending_country_choice.Bind(wx.EVT_KEY_DOWN, self.on_trending_filter_key)
        grid.Add(self.trending_country_choice, 1, wx.EXPAND)
        grid.Add(wx.StaticText(self.panel, label=self.t("trending_category")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.trending_category_choice = wx.Choice(self.panel, choices=[self.t(f"trending_{code}") for code, _label in TRENDING_CATEGORIES])
        self.trending_category_choice.SetName(self.t("trending_category"))
        selected_category = self.last_trending_category_index if category_index is None else category_index
        self.trending_category_choice.SetSelection(min(max(0, selected_category), self.trending_category_choice.GetCount() - 1))
        self.trending_category_choice.Bind(wx.EVT_CHOICE, lambda _evt: self.load_trending_results())
        self.trending_category_choice.Bind(wx.EVT_KEY_DOWN, self.on_trending_filter_key)
        grid.Add(self.trending_category_choice, 1, wx.EXPAND)
        self.root_sizer.Add(grid, 0, wx.EXPAND | wx.ALL, 4)
        self.results_list = wx.ListBox(self.panel, choices=[self.t("search_results_empty")])
        self.results_list.SetName(self.t("trending"))
        self.results_list.SetSelection(0)
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        self.focus_later(self.trending_country_choice)
        if auto_load:
            wx.CallAfter(self.load_trending_results)

    def trending_worker(self, country_code: str, category_code: str, generation: int) -> None:
        try:
            source_key = "trending_source_public"
            api_error = ""
            results: list[dict] = []
            if self.youtube_data_api_key():
                try:
                    results = self.fetch_youtube_api_trending(country_code, category_code)
                    source_key = "trending_source_api"
                except Exception as exc:
                    api_error = self.friendly_error(exc)
            if not results:
                source_key = "trending_source_public"
                results = self.fetch_public_official_trending(country_code, category_code)
            wx.CallAfter(self.show_results_if_current, generation, results)
            wx.CallAfter(self.set_status, self.t(source_key))
        except Exception as exc:
            message = self.friendly_error(exc)
            if "api_error" in locals() and api_error:
                message = f"{api_error}\n\n{message}"
            wx.CallAfter(self.show_trending_error_if_current, generation, self.t("trending_official_unavailable", error=message))

    def show_trending_error_if_current(self, generation: int, error: str) -> None:
        if generation != self.search_generation:
            return
        self.search_generation += 1
        self.message(error, wx.ICON_ERROR)
        self.announce_player(self.t("trending_unavailable_returning"))
        self.show_main_menu()

    def local_media_wildcard(self) -> str:
        patterns = ";".join(f"*{extension}" for extension in sorted(LOCAL_MEDIA_EXTENSIONS))
        return f"{self.t('media_files')} ({patterns})|{patterns}|{self.t('all_files')} (*.*)|*.*"

    @staticmethod
    def natural_sort_key(value) -> list[tuple[int, object]]:
        text = str(value or "").casefold()
        text = _RE_FILE_EXT.sub(lambda m: "\x00" + m.group(1), text)
        parts = _RE_INT_SPLIT.split(text)
        return [(1, int(part)) if part.isdigit() else (0, part) for part in parts]

    @staticmethod
    def local_folder_cache_key(folder: Path) -> str:
        try:
            return str(folder.expanduser().resolve()).lower()
        except OSError:
            return str(folder.expanduser()).lower()

    def show_play_from_folder(self) -> None:
        self.last_activated_menu_action = self.show_play_from_folder
        start_dir = self.settings.download_folder or str(Path.home())
        with wx.DirDialog(
            self,
            self.t("select_media_folder"),
            defaultPath=start_dir if Path(start_dir).exists() else str(Path.home()),
            style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                self.show_main_menu()
                return
            path = dialog.GetPath()
        self.open_local_media_folder(path)

    def show_local_media_folder(self, folder: Path, items: list[dict], selection: int = 0) -> None:
        folder_items = [dict(item) for item in items if item.get("kind") == "local_file" and item.get("url")]
        self.current_local_folder_path = str(folder)
        self.current_local_folder_items = list(folder_items)
        self.in_player_screen = False
        if not self.player_is_active():
            self.player_control_mode = False
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.folder_screen_active = True
        self.clear()
        self.add_background_player_section()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play"), self.play_selected),
                (self.t("play_folder"), lambda: self.play_local_folder(start_index=0, shuffle=False)),
                (self.t("shuffle_folder"), lambda: self.play_local_folder(start_index=0, shuffle=True)),
                (self.t("add_folder_to_queue"), self.add_local_folder_to_playback_queue),
                (self.t("playback_queue"), self.show_playback_queue),
            ]
        )
        label = wx.StaticText(self.panel, label=f"{self.t('play_from_folder')}: {folder}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.results_list = wx.ListBox(self.panel, choices=[self.t("search_results_empty")])
        self.results_list.SetName(self.t("play_from_folder"))
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.last_search_query = str(folder)
        self.last_search_type_index = 0
        self.current_search_type_code = "All"
        self.collection_url = ""
        self.collection_result_type = ""
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
        self.search_results_stack = []
        self.loading_more_results = False
        self.dynamic_fetch_enabled = False
        self.last_user_result_index = 0
        self.last_user_result_identity = ""
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        self.cache_local_folder_items(folder, folder_items)
        self.show_results(folder_items, selection=selection, visible_count=len(folder_items))
        self.set_status(self.t("folder_loaded", count=len(folder_items)))
        self.return_results = list(folder_items)
        self.return_all_results = list(folder_items)
        self.return_index = min(max(0, selection), max(0, len(folder_items) - 1))
        self.return_visible_count = len(folder_items)
        self.panel.Layout()
        self.focus_later(self.results_list)

    def clear_loading_more_if_current(self, generation: int) -> None:
        if generation == self.search_generation:
            self.loading_more_results = False

    def set_repeat_enabled(self, checked: bool, announce: bool = True) -> None:
        self.repeat_current = checked
        if self.player_kind == "mpv" and self.mpv_process_alive():
            try:
                self.mpv_set_property("loop-file", "inf" if checked else "no", timeout=0.8)
            except Exception:
                pass
        if getattr(self, "repeat_checkbox", None):
            try:
                self.repeat_checkbox.SetValue(checked)
            except RuntimeError:
                pass
        if announce:
            self.announce_player(self.t("repeat_on" if checked else "repeat_off"))

    def toggle_repeat(self) -> None:
        self.set_repeat_enabled(not self.repeat_current)

    def set_bass_boost_enabled(self, checked: bool, announce: bool = True) -> None:
        if checked == self.bass_boost_enabled:
            return
        self.session_equalizer_before_bass_boost = None
        self.bass_boost_enabled = checked
        if getattr(self, "bass_boost_checkbox", None):
            try:
                self.bass_boost_checkbox.SetValue(checked)
            except RuntimeError:
                pass
        self.schedule_equalizer_apply(30)
        if announce:
            self.announce_player(self.t("bass_boost_on" if checked else "bass_boost_off"))

    def toggle_bass_boost(self) -> None:
        self.set_bass_boost_enabled(not self.bass_boost_enabled)

    def toggle_shuffle(self) -> None:
        self.shuffle_current = not self.shuffle_current
        self.announce_player(self.t("shuffle_on" if self.shuffle_current else "shuffle_off"))

    def effective_autoplay_next(self) -> bool:
        return bool(getattr(self.settings, "autoplay_next", False) or self.session_autoplay_next)

    def apply_tab_order(self, controls: list[wx.Window]) -> None:
        previous_control = None
        for control in controls:
            live = self.live_window(control)
            if live is None:
                continue
            if previous_control is not None:
                try:
                    live.MoveAfterInTabOrder(previous_control)
                except RuntimeError:
                    pass
            previous_control = live

    def bookmark_media_key(self, item: dict | None) -> str:
        if not isinstance(item, dict):
            return ""
        for key in ("url", "webpage_url", "path", "original_url", "watch_url"):
            value = str(item.get(key) or "").strip()
            if value:
                return value
        return ""

    def bookmark_media_item(self, bookmark: dict) -> dict:
        media = bookmark.get("media") if isinstance(bookmark.get("media"), dict) else {}
        item = dict(media)
        for key in ("title", "channel", "kind", "url", "webpage_url", "path", "duration", "duration_seconds", "type"):
            if key not in item and bookmark.get(key) not in (None, ""):
                item[key] = bookmark.get(key)
        if not item.get("url"):
            item["url"] = bookmark.get("url") or bookmark.get("path") or bookmark.get("webpage_url") or ""
        if not item.get("title"):
            item["title"] = bookmark.get("media_title") or bookmark.get("name") or self.t("bookmarks")
        if not item.get("kind"):
            item["kind"] = "local_file" if self.local_media_path_from_input(str(item.get("url") or item.get("path") or "")) else "video"
        if item.get("kind") == "local_file" and not item.get("path"):
            item["path"] = item.get("url", "")
        return item

    def current_bookmark_media_item(self) -> dict:
        item = dict(self.current_player_item())
        try:
            media = self.playlist_item_from_media(item)
        except Exception:
            media = dict(item)
        for key in ("path", "original_url", "watch_url"):
            if item.get(key) and not media.get(key):
                media[key] = item.get(key)
        if not media.get("url"):
            media["url"] = item.get("url") or item.get("path") or item.get("webpage_url") or ""
        return media

    def bookmark_position(self, bookmark: dict) -> float:
        try:
            return max(0.0, float(bookmark.get("position") or 0.0))
        except (TypeError, ValueError):
            return 0.0

    def bookmark_line(self, bookmark: dict, index: int, include_media: bool = True) -> str:
        name = str(bookmark.get("name") or self.t("bookmark")).strip()
        media = bookmark.get("media") if isinstance(bookmark.get("media"), dict) else {}
        media_title = str(bookmark.get("media_title") or media.get("title") or "").strip()
        position = self.format_seconds(self.bookmark_position(bookmark))
        parts = [f"{index + 1}. {position}", name]
        if include_media and media_title:
            parts.append(media_title)
        return " | ".join(part for part in parts if part)

    def normalized_bookmarks(self) -> list[dict]:
        normalized: list[dict] = []
        changed = False
        for bookmark in list(getattr(self, "bookmarks", []) or []):
            if not isinstance(bookmark, dict):
                changed = True
                continue
            item = self.bookmark_media_item(bookmark)
            media_key = str(bookmark.get("media_key") or self.bookmark_media_key(item)).strip()
            if not media_key:
                changed = True
                continue
            copy = dict(bookmark)
            if not copy.get("id"):
                copy["id"] = f"{int(time.time() * 1000)}-{len(normalized)}"
                changed = True
            copy["media"] = item
            copy["media_key"] = media_key
            copy["position"] = round(self.bookmark_position(copy), 1)
            copy["media_title"] = str(copy.get("media_title") or item.get("title") or "").strip()
            normalized.append(copy)
        if changed:
            self.bookmarks = normalized
            self.save_bookmarks()
        return normalized

    def bookmarks_for_item(self, item: dict | None) -> list[dict]:
        media_key = self.bookmark_media_key(item)
        if not media_key:
            return []
        return sorted(
            [bookmark for bookmark in self.normalized_bookmarks() if str(bookmark.get("media_key") or "") == media_key],
            key=lambda bookmark: (self.bookmark_position(bookmark), str(bookmark.get("name") or "").casefold()),
        )

    def sorted_bookmarks(self) -> list[dict]:
        return sorted(
            self.normalized_bookmarks(),
            key=lambda bookmark: (
                str(bookmark.get("media_title") or "").casefold(),
                self.bookmark_position(bookmark),
                str(bookmark.get("name") or "").casefold(),
            ),
        )

    def add_current_bookmark(self, _event=None) -> dict | None:
        if not self.player_is_active():
            self.announce_player(self.t("no_player"))
            return None
        item = self.current_bookmark_media_item()
        media_key = self.bookmark_media_key(item)
        if not media_key:
            self.announce_player(self.t("no_selection"))
            return None
        position = float(self.current_player_position_seconds())
        default_name = self.t("bookmark_default_name", time=self.format_seconds(position))
        with wx.TextEntryDialog(self, self.t("bookmark_name_prompt"), self.t("add_bookmark"), default_name) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            name = dialog.GetValue().strip() or default_name
        bookmark = {
            "id": f"{int(time.time() * 1000)}-{random.randint(1000, 9999)}",
            "name": name,
            "position": round(position, 1),
            "media_key": media_key,
            "media_title": str(item.get("title") or self.current_player_title() or "").strip(),
            "media": item,
            "created_at": time.time(),
            "updated_at": time.time(),
        }
        self.bookmarks.append(bookmark)
        self.save_bookmarks()
        self.announce_player(self.t("bookmark_added", name=name, time=self.format_seconds(position)))
        return bookmark

    def show_player_bookmarks(self) -> None:
        if not self.ensure_player_for_auxiliary_view(self.show_player_bookmarks):
            return
        self.show_bookmarks_dialog(current_only=True)

    def show_bookmarks(self) -> None:
        self.last_activated_menu_action = self.show_bookmarks
        self.show_bookmarks_dialog(current_only=False)

    def show_bookmarks_dialog(self, current_only: bool = False) -> None:
        current_item = self.current_bookmark_media_item() if self.player_is_active() else {}
        dialog = wx.Dialog(self, title=self.t("bookmarks"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("bookmarks"))
        dialog.SetMinSize((680, 480))
        outer = wx.BoxSizer(wx.VERTICAL)
        bookmark_list = wx.ListBox(dialog, choices=[self.t("bookmarks_empty")])
        bookmark_list.SetName(self.t("bookmarks"))
        bookmark_list.SetSelection(0)
        outer.Add(bookmark_list, 1, wx.EXPAND | wx.ALL, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        add_button = wx.Button(dialog, label=self.t("add_bookmark"))
        play_button = wx.Button(dialog, label=self.t("play"))
        rename_button = wx.Button(dialog, label=self.t("rename_bookmark"))
        delete_button = wx.Button(dialog, label=self.t("delete_bookmark"))
        copy_button = wx.Button(dialog, label=self.t("copy_timestamp_link"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(add_button, 0, wx.RIGHT, 8)
        row.Add(play_button, 0, wx.RIGHT, 8)
        row.Add(rename_button, 0, wx.RIGHT, 8)
        row.Add(delete_button, 0, wx.RIGHT, 8)
        row.Add(copy_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        dialog.SetSizer(outer)
        state: dict[str, object] = {"bookmarks": [], "action": "", "bookmark": None}

        def visible_bookmarks() -> list[dict]:
            return self.bookmarks_for_item(current_item) if current_only else self.sorted_bookmarks()

        def selected_index() -> int:
            try:
                index = bookmark_list.GetSelection()
            except RuntimeError:
                return -1
            bookmarks = list(state.get("bookmarks") or [])
            return index if 0 <= index < len(bookmarks) else -1

        def selected_bookmark() -> dict | None:
            index = selected_index()
            bookmarks = list(state.get("bookmarks") or [])
            return bookmarks[index] if 0 <= index < len(bookmarks) else None

        def refresh_bookmark_list(selection: int = 0) -> None:
            bookmarks = visible_bookmarks()
            state["bookmarks"] = bookmarks
            include_media = not current_only
            labels = [self.bookmark_line(bookmark, index, include_media=include_media) for index, bookmark in enumerate(bookmarks)] or [self.t("bookmarks_empty")]
            bookmark_list.Set(labels)
            bookmark_list.SetSelection(min(max(0, selection), len(labels) - 1))
            has_bookmarks = bool(bookmarks)
            can_add = bool(self.player_is_active() and self.bookmark_media_key(current_item))
            add_button.Enable(can_add)
            play_button.Enable(has_bookmarks)
            rename_button.Enable(has_bookmarks)
            delete_button.Enable(has_bookmarks)
            copy_button.Enable(has_bookmarks)

        def play_selected(_event=None) -> None:
            bookmark = selected_bookmark()
            if not bookmark:
                self.announce_player(self.t("bookmarks_empty"))
                return
            state["action"] = "play"
            state["bookmark"] = dict(bookmark)
            dialog.EndModal(wx.ID_OK)

        def add_bookmark_from_dialog(_event=None) -> None:
            bookmark = self.add_current_bookmark()
            if bookmark:
                refresh_bookmark_list(len(visible_bookmarks()) - 1)

        def rename_selected(_event=None) -> None:
            bookmark = selected_bookmark()
            if not bookmark:
                return
            current_name = str(bookmark.get("name") or self.t("bookmark")).strip()
            with wx.TextEntryDialog(self, self.t("bookmark_name_prompt"), self.t("rename_bookmark"), current_name) as name_dialog:
                if name_dialog.ShowModal() != wx.ID_OK:
                    return
                new_name = name_dialog.GetValue().strip()
            if not new_name:
                return
            target_id = str(bookmark.get("id") or "")
            for stored in self.bookmarks:
                if str(stored.get("id") or "") == target_id:
                    stored["name"] = new_name
                    stored["updated_at"] = time.time()
                    break
            self.save_bookmarks()
            refresh_bookmark_list(selected_index())
            self.announce_player(self.t("bookmark_renamed", name=new_name))

        def delete_selected(_event=None) -> None:
            bookmark = selected_bookmark()
            if not bookmark:
                return
            target_id = str(bookmark.get("id") or "")
            index = selected_index()
            self.bookmarks = [stored for stored in self.bookmarks if str(stored.get("id") or "") != target_id]
            self.save_bookmarks()
            refresh_bookmark_list(index)
            self.announce_player(self.t("bookmark_deleted"))

        def copy_selected_timestamp(_event=None) -> None:
            bookmark = selected_bookmark()
            if not bookmark:
                return
            item = self.bookmark_media_item(bookmark)
            url = self.youtube_url_at_timestamp(item, int(self.bookmark_position(bookmark)))
            if not url:
                self.announce_player(self.t("timestamp_url_unavailable"))
                return
            self.copy_plain_text_to_clipboard(url)
            self.announce_player(self.t("timestamp_url_copied"))

        def open_bookmark_context_menu(_event=None) -> None:
            menu = wx.Menu()
            actions = [
                (self.t("play"), play_selected),
                (self.t("add_bookmark"), add_bookmark_from_dialog),
                (self.t("rename_bookmark"), rename_selected),
                (self.t("delete_bookmark"), delete_selected),
                (self.t("copy_timestamp_link"), copy_selected_timestamp),
            ]
            for label, handler in actions:
                menu_item = menu.Append(wx.ID_ANY, label)
                dialog.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
            bookmark_list.PopupMenu(menu)
            menu.Destroy()

        def on_bookmark_key(event: wx.KeyEvent) -> None:
            if self.shortcut_matches(event, "open_selected"):
                play_selected()
                return
            if self.shortcut_matches(event, "remove_selected"):
                delete_selected()
                return
            if self.context_menu_shortcut_matches(event):
                open_bookmark_context_menu(event)
                return
            if self.shortcut_matches(event, "player_back"):
                dialog.EndModal(wx.ID_CANCEL)
                return
            event.Skip()

        bookmark_list.Bind(wx.EVT_LISTBOX_DCLICK, play_selected)
        bookmark_list.Bind(wx.EVT_CONTEXT_MENU, open_bookmark_context_menu)
        bookmark_list.Bind(wx.EVT_KEY_DOWN, on_bookmark_key)
        play_button.Bind(wx.EVT_BUTTON, play_selected)
        add_button.Bind(wx.EVT_BUTTON, add_bookmark_from_dialog)
        rename_button.Bind(wx.EVT_BUTTON, rename_selected)
        delete_button.Bind(wx.EVT_BUTTON, delete_selected)
        copy_button.Bind(wx.EVT_BUTTON, copy_selected_timestamp)
        refresh_bookmark_list(0)
        result = dialog.ShowModal()
        action = str(state.get("action") or "")
        bookmark = dict(state.get("bookmark") or {})
        dialog.Destroy()
        if result == wx.ID_OK and action == "play" and bookmark:
            self.play_bookmark(bookmark)

    def seek_to_bookmark(self, bookmark: dict) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            self.announce_player(self.t("no_player"))
            return
        position = self.bookmark_position(bookmark)
        try:
            self.cancel_clip_preview()
            self.mpv_send(["seek", position, "absolute+exact"], timeout=0.8)
            self.announce_player(self.t("bookmark_selected", name=str(bookmark.get("name") or self.t("bookmark")), time=self.format_seconds(position)))
        except Exception:
            self.announce_player(self.t("timing_unavailable"))

    def play_bookmark(self, bookmark: dict) -> None:
        item = self.bookmark_media_item(bookmark)
        url = str(item.get("url") or item.get("path") or item.get("webpage_url") or "").strip()
        if not url:
            self.announce_player(self.t("no_selection"))
            return
        if self.player_is_active() and self.bookmark_media_key(item) == self.bookmark_media_key(self.current_player_item()):
            self.seek_to_bookmark(bookmark)
            return
        item["_bookmark_start_position"] = self.bookmark_position(bookmark)
        self.clear_player_sequence()
        self.return_results = []
        self.return_all_results = []
        self.return_visible_count = 0
        self.return_index = 0
        self.player_return_screen = "bookmarks"
        self.player_return_data = {}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(url, str(item.get("title") or bookmark.get("media_title") or bookmark.get("name") or ""))

    def show_chapters(self) -> None:
        if not self.ensure_player_for_auxiliary_view(self.show_chapters):
            return
        chapters = self.current_chapters()
        if not chapters:
            self.announce_player(self.t("no_chapters_available"))
            return
        dialog = wx.Dialog(self, title=self.t("chapters"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("chapters"))
        dialog.SetMinSize((560, 420))
        outer = wx.BoxSizer(wx.VERTICAL)
        chapter_list = wx.ListBox(dialog, choices=[self.chapter_line(chapter, index) for index, chapter in enumerate(chapters)])
        chapter_list.SetName(self.t("chapter_list"))
        chapter_list.SetSelection(max(0, self.current_chapter_index(chapters)))
        outer.Add(chapter_list, 1, wx.EXPAND | wx.ALL, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        play_button = wx.Button(dialog, label=self.t("play"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(play_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)

        def selected_index() -> int:
            try:
                index = chapter_list.GetSelection()
            except RuntimeError:
                return -1
            return index if 0 <= index < len(chapters) else -1

        def play_selected(_event=None) -> None:
            index = selected_index()
            if index >= 0:
                self.seek_to_chapter(chapters[index])
                dialog.EndModal(wx.ID_OK)

        def on_chapter_key(event: wx.KeyEvent) -> None:
            key_code = event.GetKeyCode()
            if self.shortcut_matches(event, "open_selected") or key_code in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
                play_selected()
                return
            if self.shortcut_matches(event, "player_back"):
                dialog.EndModal(wx.ID_CANCEL)
                return
            event.Skip()

        chapter_list.Bind(wx.EVT_LISTBOX_DCLICK, play_selected)
        chapter_list.Bind(wx.EVT_KEY_DOWN, on_chapter_key)
        dialog.Bind(wx.EVT_CHAR_HOOK, on_chapter_key)
        play_button.Bind(wx.EVT_BUTTON, play_selected)
        try:
            play_button.SetDefault()
        except RuntimeError:
            pass
        dialog.ShowModal()
        dialog.Destroy()
        self.focus_player_target_later("player")

    def seek_to_chapter(self, chapter: dict) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            self.announce_player(self.t("no_player"))
            return
        try:
            start = max(0.0, float(chapter.get("start_time") or 0.0))
            self.cancel_clip_preview()
            self.mpv_send(["seek", start, "absolute+exact"], timeout=0.8)
            title = str(chapter.get("title") or self.t("chapters"))
            self.announce_player(self.t("chapter_selected", title=title, time=self.format_seconds(start)))
        except Exception:
            self.announce_player(self.t("timing_unavailable"))

    def seek_relative_chapter(self, delta: int) -> None:
        chapters = self.current_chapters()
        if not chapters:
            self.announce_player(self.t("no_chapters_available"))
            return
        try:
            position = float(self.mpv_get_property("time-pos", timeout=0.35) or 0.0)
        except Exception:
            position = 0.0
        target_index = -1
        if delta > 0:
            for index, chapter in enumerate(chapters):
                if float(chapter.get("start_time") or 0.0) > position + 0.75:
                    target_index = index
                    break
        else:
            previous = [index for index, chapter in enumerate(chapters) if float(chapter.get("start_time") or 0.0) < position - 1.5]
            target_index = previous[-1] if previous else 0
        if target_index < 0 or target_index >= len(chapters):
            self.announce_player(self.t("no_chapters_available"))
            return
        self.seek_to_chapter(chapters[target_index])

    def show_transcript(self) -> None:
        if not self.ensure_player_for_auxiliary_view(self.show_transcript):
            return
        source_item = dict(self.current_video_item or {})
        if isinstance(self.current_video_info, dict):
            for key, value in self.current_video_info.items():
                if key not in source_item:
                    source_item[key] = value
        dialog = wx.Dialog(self, title=self.t("transcript"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("transcript"))
        dialog.SetMinSize((700, 520))
        outer = wx.BoxSizer(wx.VERTICAL)
        search_box = wx.TextCtrl(dialog, style=wx.TE_PROCESS_ENTER | wx.WANTS_CHARS)
        search_box.SetName(self.t("transcript_search"))
        outer.Add(search_box, 0, wx.EXPAND | wx.LEFT | wx.RIGHT | wx.TOP, 8)
        transcript_list = wx.ListBox(dialog, choices=[self.t("transcript_loading")])
        transcript_list.SetName(self.t("transcript"))
        transcript_list.SetSelection(0)
        outer.Add(transcript_list, 1, wx.EXPAND | wx.ALL, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        jump_button = wx.Button(dialog, label=self.t("play"))
        copy_line_button = wx.Button(dialog, label=self.t("copy_transcript_line"))
        copy_all_button = wx.Button(dialog, label=self.t("copy_transcript"))
        copy_time_button = wx.Button(dialog, label=self.t("copy_timestamp_link"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(jump_button, 0, wx.RIGHT, 8)
        row.Add(copy_line_button, 0, wx.RIGHT, 8)
        row.Add(copy_all_button, 0, wx.RIGHT, 8)
        row.Add(copy_time_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        dialog.SetSizer(outer)
        jump_button.Enable(False)
        copy_line_button.Enable(False)
        copy_all_button.Enable(False)
        copy_time_button.Enable(False)
        state: dict[str, object] = {"entries": [], "filtered": [], "source_key": "", "loading": True}

        def selected_entry() -> tuple[int, dict] | None:
            filtered = list(state.get("filtered") or [])
            try:
                selection = transcript_list.GetSelection()
            except RuntimeError:
                return None
            if 0 <= selection < len(filtered):
                original_index, entry = filtered[selection]
                if isinstance(entry, dict):
                    return int(original_index), entry
            return None

        def refresh_transcript(selection: int = 0) -> None:
            entries = list(state.get("entries") or [])
            query = search_box.GetValue().strip().lower()
            filtered: list[tuple[int, dict]] = []
            for index, entry in enumerate(entries):
                line = self.transcript_line(entry, index)
                if not query or query in line.lower():
                    filtered.append((index, entry))
            state["filtered"] = filtered
            if filtered:
                labels = [self.transcript_line(entry, index) for index, entry in filtered]
            elif entries:
                labels = [self.t("transcript_no_search_results")]
            else:
                labels = [self.t("no_transcript_available")]
            transcript_list.Set(labels)
            transcript_list.SetSelection(min(max(0, selection), len(labels) - 1))
            has_entries = bool(filtered)
            jump_button.Enable(has_entries)
            copy_line_button.Enable(has_entries)
            copy_all_button.Enable(bool(entries))
            copy_time_button.Enable(has_entries and bool(self.youtube_url_at_timestamp(source_item, 0)))

        def finish_load(entries: list[dict], source_key: str = "", error: str = "") -> None:
            try:
                state["loading"] = False
                if error:
                    transcript_list.Set([self.t("transcript_failed", error=error)])
                    transcript_list.SetSelection(0)
                    self.announce_player(self.t("transcript_failed", error=error))
                    return
                state["entries"] = list(entries or [])
                state["source_key"] = source_key
                refresh_transcript(0)
                if entries:
                    source = self.t(source_key) if source_key else ""
                    message = self.t("transcript_loaded_from_source", count=len(entries), source=source) if source else self.t("transcript_loaded", count=len(entries))
                    self.announce_player(message)
                else:
                    self.announce_player(self.t("no_transcript_available"))
            except RuntimeError:
                pass

        def seek_selected(_event=None) -> None:
            selected = selected_entry()
            if not selected:
                self.announce_player(self.t("no_transcript_available"))
                return
            _index, entry = selected
            if self.player_kind != "mpv" or not self.mpv_process_alive():
                self.announce_player(self.t("no_player"))
                return
            try:
                start = max(0.0, float(entry.get("start") or 0.0))
                self.cancel_clip_preview()
                self.mpv_send(["seek", start, "absolute+exact"], timeout=0.8)
                text = str(entry.get("text") or "").strip()
                if len(text) > 90:
                    text = text[:87].rstrip() + "..."
                self.announce_player(self.t("transcript_selected", time=self.format_seconds(start), text=text))
            except Exception:
                self.announce_player(self.t("timing_unavailable"))

        def copy_selected_line(_event=None) -> None:
            selected = selected_entry()
            if not selected:
                self.announce_player(self.t("no_transcript_available"))
                return
            index, entry = selected
            self.copy_plain_text_to_clipboard(self.transcript_line(entry, index))
            self.announce_player(self.t("transcript_line_copied"))

        def copy_full_transcript(_event=None) -> None:
            entries = list(state.get("entries") or [])
            if not entries:
                self.announce_player(self.t("no_transcript_available"))
                return
            self.copy_plain_text_to_clipboard(self.transcript_full_text(entries))
            self.announce_player(self.t("transcript_copied"))

        def copy_selected_timestamp(_event=None) -> None:
            selected = selected_entry()
            if not selected:
                self.announce_player(self.t("no_transcript_available"))
                return
            _index, entry = selected
            url = self.youtube_url_at_timestamp(source_item, int(float(entry.get("start") or 0.0)))
            if not url:
                self.announce_player(self.t("timestamp_url_unavailable"))
                return
            self.copy_plain_text_to_clipboard(url)
            self.announce_player(self.t("timestamp_url_copied"))

        def on_search_changed(_event=None) -> None:
            refresh_transcript(0)

        def on_transcript_key(event: wx.KeyEvent) -> None:
            if self.shortcut_matches(event, "player_back"):
                dialog.EndModal(wx.ID_CANCEL)
                return
            if self.shortcut_matches(event, "open_selected"):
                focused = wx.Window.FindFocus()
                if focused in (jump_button, copy_line_button, copy_all_button, copy_time_button, close_button):
                    event.Skip()
                    return
                seek_selected()
                return
            event.Skip()

        search_box.Bind(wx.EVT_TEXT, on_search_changed)
        transcript_list.Bind(wx.EVT_LISTBOX_DCLICK, seek_selected)
        transcript_list.Bind(wx.EVT_KEY_DOWN, on_transcript_key)
        dialog.Bind(wx.EVT_CHAR_HOOK, on_transcript_key)
        jump_button.Bind(wx.EVT_BUTTON, seek_selected)
        copy_line_button.Bind(wx.EVT_BUTTON, copy_selected_line)
        copy_all_button.Bind(wx.EVT_BUTTON, copy_full_transcript)
        copy_time_button.Bind(wx.EVT_BUTTON, copy_selected_timestamp)
        threading.Thread(target=self.fetch_transcript_worker, args=(source_item, finish_load), daemon=True).start()
        wx.CallAfter(search_box.SetFocus)
        dialog.ShowModal()
        dialog.Destroy()
        self.focus_player_target_later("player")

    def show_lyrics(self) -> None:
        if not self.ensure_player_for_auxiliary_view(self.show_lyrics):
            return
        dialog = wx.Dialog(self, title=self.t("lyrics"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("lyrics"))
        dialog.SetMinSize((620, 460))
        outer = wx.BoxSizer(wx.VERTICAL)
        lyrics_text = wx.TextCtrl(dialog, value=self.t("lyrics_fetching"), style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2 | wx.VSCROLL | wx.HSCROLL | wx.WANTS_CHARS)
        lyrics_text.SetName(self.t("lyrics"))
        outer.Add(lyrics_text, 1, wx.EXPAND | wx.ALL, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        copy_button = wx.Button(dialog, label=self.t("copy_lyrics"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(copy_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        dialog.SetSizer(outer)

        dialog.lyrics_lines = []
        dialog.active_line_index = -1
        dialog.lyrics_timer = wx.Timer(dialog)

        def on_lyrics_timer(event):
            if not dialog.lyrics_lines:
                return
            try:
                pos = self.mpv_get_property("time-pos", timeout=0.1)
                if pos is None:
                    return
                pos = float(pos)
                
                current_idx = -1
                for i, (t, txt, start, end) in enumerate(dialog.lyrics_lines):
                    if pos >= t:
                        current_idx = i
                    else:
                        break
                        
                if current_idx != dialog.active_line_index:
                    lyrics_text.SetStyle(0, lyrics_text.GetLastPosition(), wx.TextAttr(wx.NullColour, lyrics_text.GetBackgroundColour()))
                    dialog.active_line_index = current_idx
                    if current_idx >= 0:
                        _, _, start, end = dialog.lyrics_lines[current_idx]
                        lyrics_text.SetStyle(start, end, wx.TextAttr(wx.NullColour, wx.Colour(173, 216, 230)))
                        lyrics_text.ShowPosition(start)
            except Exception:
                pass
                
        dialog.Bind(wx.EVT_TIMER, on_lyrics_timer, dialog.lyrics_timer)

        def set_lyrics_text(text: str, source: str = "") -> None:
            try:
                lines = text.strip().split('\n') if text else []
                parsed_lines = []
                clean_text = ""
                
                header = f"{source}\n\n" if source and text.strip() else ""
                clean_text += header
                current_pos = len(clean_text)
                
                for line in lines:
                    line = line.strip()
                    if not line:
                        continue
                    match = _RE_LRC_LINE.match(line)
                    if match:
                        minutes = int(match.group(1))
                        seconds = float(match.group(2))
                        lyric_text = match.group(3).strip()
                        if lyric_text:
                            time_sec = minutes * 60 + seconds
                            start_idx = current_pos
                            clean_text += lyric_text + "\n"
                            end_idx = current_pos + len(lyric_text)
                            parsed_lines.append((time_sec, lyric_text, start_idx, end_idx))
                            current_pos = end_idx + 1
                    else:
                        start_idx = current_pos
                        clean_text += line + "\n"
                        end_idx = current_pos + len(line)
                        current_pos = end_idx + 1
                
                if parsed_lines:
                    dialog.lyrics_lines = parsed_lines
                    dialog.lyrics_timer.Start(200)
                
                value = clean_text.strip() if clean_text.strip() else self.t("no_lyrics_available")
                lyrics_text.SetValue(value)
                lyrics_text.SetInsertionPoint(0)
                lyrics_text.SetFocus()
                self.announce_player(self.t("lyrics") if text else self.t("no_lyrics_available"))
            except RuntimeError:
                pass

        local_lyrics = self.local_lyrics_text()
        if local_lyrics:
            wx.CallAfter(set_lyrics_text, local_lyrics, self.t("lyrics_source_local"))
        elif bool(getattr(self.settings, "enable_online_lyrics", True)):
            threading.Thread(target=self.fetch_lyrics_worker, args=(self.lyrics_search_terms(), set_lyrics_text), daemon=True).start()
        else:
            wx.CallAfter(set_lyrics_text, "", "")

        def copy_current_lyrics(_event=None) -> None:
            text = lyrics_text.GetValue().strip()
            if not text or text in {self.t("lyrics_fetching"), self.t("no_lyrics_available")}:
                self.announce_player(self.t("no_lyrics_available"))
                return
            self.copy_plain_text_to_clipboard(text)
            self.announce_player(self.t("lyrics_copied"))

        copy_button.Bind(wx.EVT_BUTTON, copy_current_lyrics)
            
        dialog.ShowModal()
        dialog.lyrics_timer.Stop()
        dialog.Destroy()
        self.focus_player_target_later("player")

    def show_comments(self) -> None:
        if not self.ensure_player_for_auxiliary_view(self.show_comments):
            return
        source_item = self.current_video_info or self.current_video_item or {}
        video_id = self.extract_youtube_video_id(source_item)
        if not video_id:
            self.announce_player(self.t("comments_disabled"))
            return
        source_url = self.youtube_comments_source_url(source_item, video_id)
        dialog = wx.Dialog(self, title=self.t("comments"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("comments"))
        dialog.SetMinSize((820, 520))
        outer = wx.BoxSizer(wx.VERTICAL)

        filter_row = wx.BoxSizer(wx.HORIZONTAL)
        search_box = wx.TextCtrl(dialog, value="", style=wx.TE_PROCESS_ENTER)
        search_box.SetName(self.t("search_comments"))
        sort_choices = [
            ("relevance", self.t("comments_sort_relevance")),
            ("newest", self.t("comments_sort_newest")),
            ("oldest", self.t("comments_sort_oldest")),
            ("likes", self.t("comments_sort_likes")),
            ("replies", self.t("comments_sort_replies")),
        ]
        sort_box = wx.Choice(dialog, choices=[label for _value, label in sort_choices])
        sort_box.SetName(self.t("comments_sort"))
        sort_box.SetSelection(0)
        filter_row.Add(search_box, 1, wx.RIGHT, 8)
        filter_row.Add(sort_box, 0)
        outer.Add(filter_row, 0, wx.EXPAND | wx.LEFT | wx.RIGHT | wx.TOP, 8)

        comments_list = wx.ListBox(dialog, choices=[self.t("comments_loading")])
        comments_list.SetName(self.t("comments"))
        comments_list.SetSelection(0)
        outer.Add(comments_list, 1, wx.EXPAND | wx.ALL, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        open_button = wx.Button(dialog, label=self.t("open_comment"))
        copy_button = wx.Button(dialog, label=self.t("copy_comment"))
        copy_all_button = wx.Button(dialog, label=self.t("copy_visible_comments"))
        author_button = wx.Button(dialog, label=self.t("open_comment_author_channel"))
        more_button = wx.Button(dialog, label=self.t("load_more_comments"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(open_button, 0, wx.RIGHT, 8)
        row.Add(copy_button, 0, wx.RIGHT, 8)
        row.Add(copy_all_button, 0, wx.RIGHT, 8)
        row.Add(author_button, 0, wx.RIGHT, 8)
        row.Add(more_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        dialog.SetSizer(outer)
        open_button.Enable(False)
        copy_button.Enable(False)
        copy_all_button.Enable(False)
        author_button.Enable(False)
        more_button.Enable(False)
        state: dict[str, object] = {
            "comments": [],
            "visible_comments": [],
            "next_page": "",
            "loading": False,
            "loaded_once": False,
            "source_key": "",
            "sort": "relevance",
        }

        def visible_comments() -> list[dict]:
            query = ""
            try:
                query = search_box.GetValue()
            except RuntimeError:
                query = ""
            comments = [comment for comment in list(state.get("comments") or []) if self.comment_matches_query(comment, query)]
            return self.comments_sorted(comments, str(state.get("sort") or "relevance"))

        def selected_comment() -> dict | None:
            comments = list(state.get("visible_comments") or [])
            try:
                index = comments_list.GetSelection()
            except RuntimeError:
                index = -1
            if 0 <= index < len(comments):
                return comments[index]
            return None

        def refresh_comments(selection: int = 0) -> None:
            comments = visible_comments()
            state["visible_comments"] = comments
            query_active = False
            try:
                query_active = bool(search_box.GetValue().strip())
            except RuntimeError:
                query_active = False
            placeholder = self.t("no_matching_comments") if query_active and state.get("comments") else self.t("comments_disabled")
            labels = [self.comment_line(comment, index) for index, comment in enumerate(comments)] or [placeholder]
            comments_list.Set(labels)
            comments_list.SetSelection(min(max(0, selection), len(labels) - 1))
            open_button.Enable(bool(comments))
            copy_button.Enable(bool(comments))
            copy_all_button.Enable(bool(comments))
            selected_index = comments_list.GetSelection()
            author_button.Enable(bool(0 <= selected_index < len(comments) and self.comment_author_channel_url(comments[selected_index])))
            more_button.Enable(bool(state.get("next_page")) and not bool(state.get("loading")))

        def finish_load(new_comments: list[dict], next_page: str, error: str = "", source_key: str = "") -> None:
            try:
                state["loading"] = False
                if error:
                    comments_list.Set([self.t("comments_failed", error=error)])
                    comments_list.SetSelection(0)
                    more_button.Enable(False)
                    self.announce_player(self.t("comments_failed", error=error))
                    return
                existing = list(state.get("comments") or [])
                state["comments"] = existing + list(new_comments or [])
                state["next_page"] = next_page
                if source_key:
                    state["source_key"] = source_key
                state["loaded_once"] = True
                refresh_comments(len(existing) if existing else 0)
                total = len(state.get("comments") or [])
                if total:
                    source = self.t(str(state.get("source_key") or ""))
                    message = self.t("comments_loaded_from_source", count=total, source=source) if source else self.t("comments_loaded", count=total)
                else:
                    message = self.t("comments_disabled")
                self.announce_player(message)
            except RuntimeError:
                pass

        def load_more(_event=None) -> None:
            if state.get("loading"):
                return
            if (state.get("comments") or state.get("loaded_once")) and not state.get("next_page"):
                self.announce_player(self.t("no_more_comments"))
                return
            state["loading"] = True
            more_button.Enable(False)
            if not state.get("comments"):
                comments_list.Set([self.t("comments_loading")])
                comments_list.SetSelection(0)
            page_token = str(state.get("next_page") or "")
            threading.Thread(target=self.fetch_comments_worker, args=(video_id, page_token, source_url, finish_load), daemon=True).start()

        def open_selected_comment(_event=None) -> None:
            comment = selected_comment()
            if comment:
                self.show_comment_details(comment)

        def copy_selected_comment(_event=None) -> None:
            comment = selected_comment()
            if not comment:
                self.announce_player(self.t("no_matching_comments") if search_box.GetValue().strip() else self.t("comments_disabled"))
                return
            self.copy_plain_text_to_clipboard(self.comment_copy_text(comment))
            self.announce_player(self.t("comment_copied"))

        def copy_visible_comments(_event=None) -> None:
            comments = list(state.get("visible_comments") or [])
            if not comments:
                self.announce_player(self.t("no_matching_comments") if search_box.GetValue().strip() else self.t("comments_disabled"))
                return
            self.copy_plain_text_to_clipboard(self.comments_copy_text(comments))
            self.announce_player(self.t("comments_copied", count=len(comments)))

        def open_author_channel(_event=None) -> None:
            comment = selected_comment()
            url = self.comment_author_channel_url(comment or {})
            if not url:
                self.announce_player(self.t("comment_author_channel_unavailable"))
                return
            import_module("webbrowser").open(url)
            self.announce_player(self.t("comment_author_channel_opened"))

        def on_filter_changed(_event=None) -> None:
            refresh_comments(0)

        def on_sort_changed(_event=None) -> None:
            selection = sort_box.GetSelection()
            if 0 <= selection < len(sort_choices):
                state["sort"] = sort_choices[selection][0]
            refresh_comments(0)

        def on_comment_selection_changed(event: wx.CommandEvent) -> None:
            author_button.Enable(bool(self.comment_author_channel_url(selected_comment() or {})))
            event.Skip()

        def on_comments_key(event: wx.KeyEvent) -> None:
            if self.shortcut_matches(event, "open_selected"):
                open_selected_comment()
                return
            if self.shortcut_matches(event, "player_back"):
                dialog.EndModal(wx.ID_CANCEL)
                return
            event.Skip()

        def show_comments_context_menu(_event=None) -> None:
            menu = wx.Menu()
            open_id = wx.NewIdRef()
            copy_id = wx.NewIdRef()
            copy_all_id = wx.NewIdRef()
            author_id = wx.NewIdRef()
            more_id = wx.NewIdRef()
            menu.Append(open_id, self.t("open_comment"))
            menu.Append(copy_id, self.t("copy_comment"))
            menu.Append(copy_all_id, self.t("copy_visible_comments"))
            menu.Append(author_id, self.t("open_comment_author_channel"))
            menu.AppendSeparator()
            menu.Append(more_id, self.t("load_more_comments"))
            menu.Enable(open_id, bool(state.get("visible_comments")))
            menu.Enable(copy_id, bool(state.get("visible_comments")))
            menu.Enable(copy_all_id, bool(state.get("visible_comments")))
            menu.Enable(author_id, bool(self.comment_author_channel_url(selected_comment() or {})))
            menu.Enable(more_id, bool(state.get("next_page")) and not bool(state.get("loading")))
            dialog.Bind(wx.EVT_MENU, lambda _event: open_selected_comment(), id=open_id)
            dialog.Bind(wx.EVT_MENU, lambda _event: copy_selected_comment(), id=copy_id)
            dialog.Bind(wx.EVT_MENU, lambda _event: copy_visible_comments(), id=copy_all_id)
            dialog.Bind(wx.EVT_MENU, lambda _event: open_author_channel(), id=author_id)
            dialog.Bind(wx.EVT_MENU, lambda _event: load_more(), id=more_id)
            comments_list.PopupMenu(menu)
            menu.Destroy()

        search_box.Bind(wx.EVT_TEXT, on_filter_changed)
        sort_box.Bind(wx.EVT_CHOICE, on_sort_changed)
        comments_list.Bind(wx.EVT_LISTBOX, on_comment_selection_changed)
        comments_list.Bind(wx.EVT_LISTBOX_DCLICK, open_selected_comment)
        comments_list.Bind(wx.EVT_KEY_DOWN, on_comments_key)
        comments_list.Bind(wx.EVT_CONTEXT_MENU, show_comments_context_menu)
        open_button.Bind(wx.EVT_BUTTON, open_selected_comment)
        copy_button.Bind(wx.EVT_BUTTON, copy_selected_comment)
        copy_all_button.Bind(wx.EVT_BUTTON, copy_visible_comments)
        author_button.Bind(wx.EVT_BUTTON, open_author_channel)
        more_button.Bind(wx.EVT_BUTTON, load_more)
        load_more()
        wx.CallAfter(search_box.SetFocus)
        dialog.ShowModal()
        dialog.Destroy()
        self.focus_player_target_later("player")

    def show_comment_details(self, comment: dict) -> None:
        details = self.comment_details_text(comment)
        dialog = wx.Dialog(self, title=self.t("comment_details"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("comment_details"))
        dialog.SetMinSize((620, 420))
        outer = wx.BoxSizer(wx.VERTICAL)
        text = wx.TextCtrl(dialog, value=details, style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2 | wx.VSCROLL | wx.HSCROLL | wx.WANTS_CHARS)
        text.SetName(self.t("comment_details"))
        outer.Add(text, 1, wx.EXPAND | wx.ALL, 8)
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        outer.Add(close_button, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        dialog.SetSizer(outer)
        wx.CallAfter(text.SetFocus)
        dialog.ShowModal()
        dialog.Destroy()

    def show_output_devices(self) -> None:
        if not self.player_is_active():
            return
        try:
            devices = self.mpv_get_property("audio-device-list", timeout=1.5) or []
        except Exception:
            devices = []
        choices: list[str] = []
        values: list[str] = []
        for device in devices:
            if not isinstance(device, dict):
                continue
            name = str(device.get("name") or "").strip()
            if not name:
                continue
            description = str(device.get("description") or name).strip()
            choices.append(f"{description} ({name})" if description != name else name)
            values.append(name)
        if not choices:
            self.announce_player(self.t("no_output_devices"))
            return
        with wx.SingleChoiceDialog(self, self.t("select_output_device"), self.t("output_devices"), choices) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            index = dialog.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(values):
            return
        value = values[index]
        try:
            self.mpv_set_property("audio-device", value)
            self.session_audio_output_device = value
            self.current_audio_device = value
            if self.session_equalizer_enabled is None:
                self.schedule_equalizer_apply(30)
            self.announce_player(self.t("output_device_set", device=choices[index]))
        except Exception as exc:
            self.announce_player(self.t("stream_url_failed", error=self.friendly_error(exc)))

    def toggle_edit_mode(self) -> None:
        if not self.player_is_active():
            return
        if not self.current_local_media_path():
            self.announce_player(self.t("edit_mode_local_only"))
            return
        self.edit_mode_enabled = not self.edit_mode_enabled
        self.announce_player(self.t("edit_mode_on" if self.edit_mode_enabled else "edit_mode_off"))

    def current_speed_value(self) -> float:
        try:
            return self.parse_rate_value(self.current_video_info.get("speed") or self.settings.player_speed or 1.0)
        except (TypeError, ValueError):
            return 1.0

    @staticmethod
    def ffmpeg_atempo_chain(factor: float) -> list[str]:
        values: list[str] = []
        factor = max(0.0625, min(16.0, float(factor or 1.0)))
        while factor < 0.5:
            values.append("atempo=0.5")
            factor /= 0.5
        while factor > 2.0:
            values.append("atempo=2.0")
            factor /= 2.0
        if abs(factor - 1.0) >= 0.001:
            values.append(f"atempo={factor:.6f}")
        return values

    def local_edit_ffmpeg_args(self, ffmpeg: str, source: Path, output: Path) -> list[str]:
        speed = max(0.25, min(4.0, self.current_speed_value()))
        audio_filters = self.local_edit_audio_filters()
        args = [ffmpeg, "-y", "-hide_banner", "-loglevel", "error", "-i", str(source)]
        if audio_filters:
            args.extend(["-af", ",".join(audio_filters)])
            args.extend(self.local_edit_audio_codec_args(output.suffix))
        else:
            args.extend(["-c:a", "copy"])
        if self.is_video_file_extension(source):
            if abs(speed - 1.0) >= 0.001:
                args.extend(["-vf", f"setpts={1.0 / speed:.8f}*PTS", "-c:v", "libx264", "-preset", "veryfast", "-crf", "18"])
            else:
                args.extend(["-c:v", "copy"])
        args.append(str(output))
        return args

    def play_next_standard_fallback(self, allow_standard: bool = True, announce_no_related: bool = False) -> None:
        if allow_standard:
            sequence_active = self.current_player_sequence_active()
            if not sequence_active:
                queued_item = self.pop_next_playback_queue_item()
                if queued_item:
                    self.open_playback_queue_item_with_mode(queued_item, show_player=self.in_player_screen or not self.background_playback_enabled())
                    return
            next_item = self.relative_player_item(1)
            if next_item:
                self.open_relative_player_item(next_item)
                return
            if sequence_active:
                queued_item = self.pop_next_playback_queue_item()
                if queued_item:
                    self.open_playback_queue_item_with_mode(queued_item, show_player=self.in_player_screen or not self.background_playback_enabled())
                    return
        self.player_ended = True
        self.player_paused = True
        self.update_play_pause_buttons()
        if announce_no_related:
            self.announce_player(self.t("no_related_video"))
        elif bool(getattr(self.settings, "announce_playback_finished", True)):
            self.announce_player(self.t("playback_finished"))
        else:
            self.set_status(self.t("playback_finished"))

    def announce_current_play_pause_state(self) -> None:
        if not self.settings.announce_play_pause or self.player_kind != "mpv" or not self.mpv_process_alive():
            return
        try:
            paused = bool(self.mpv_get_property("pause", timeout=0.35))
            self.player_paused = paused
            self.update_play_pause_buttons()
            self.announce_play_pause_state(paused)
        except Exception:
            pass

    def announce_play_pause_state(self, paused: bool) -> None:
        if not self.settings.announce_play_pause:
            return
        # When a play/pause button has focus, the screen reader will read the
        # new button label automatically (SetLabel fires EVENT_OBJECT_NAMECHANGE).
        # A separate speakText call on top of that produces a double announcement
        # ("Paused." then "Play", or vice versa).  Skip the explicit announce and
        # rely on the button label change for SR feedback in that case.
        focus = wx.Window.FindFocus()
        play_pause_buttons = getattr(self, "player_play_pause_buttons", [])
        if focus is not None and any(focus is b for b in play_pause_buttons):
            return
        self.announce_player(self.t("playback_paused" if paused else "playback_playing"))

    def set_clip_marker_async(self, marker: str) -> None:
        self.cancel_clip_preview()
        threading.Thread(target=self.set_clip_marker_worker, args=(marker,), daemon=True).start()

    def set_clip_marker_worker(self, marker: str) -> None:
        try:
            if marker == "start" and self.clip_start_marker is not None:
                self.clip_start_marker = None
                wx.CallAfter(self.announce_player, self.t("clip_start_marker_cleared"))
                return
            if marker == "end" and self.clip_end_marker is not None:
                self.clip_end_marker = None
                wx.CallAfter(self.announce_player, self.t("clip_end_marker_cleared"))
                return
            elapsed = self.mpv_get_property("time-pos")
            if elapsed is None:
                wx.CallAfter(self.announce_player, self.t("timing_unavailable"))
                return
            position = max(0.0, float(elapsed))
            if marker == "start":
                self.clip_start_marker = position
                wx.CallAfter(self.announce_player, self.t("clip_start_marker_set", time=self.format_seconds(position)))
            else:
                self.clip_end_marker = position
                wx.CallAfter(self.announce_player, self.t("clip_end_marker_set", time=self.format_seconds(position)))
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def clip_markers_are_set(self) -> bool:
        return self.clip_start_marker is not None and self.clip_end_marker is not None

    def clip_markers_partially_set(self) -> bool:
        return (self.clip_start_marker is None) != (self.clip_end_marker is None)

    def marked_clip_range(self) -> tuple[float, float] | None:
        if self.clip_start_marker is None or self.clip_end_marker is None:
            self.announce_player(self.t("clip_markers_missing"))
            return None
        start = float(self.clip_start_marker)
        end = float(self.clip_end_marker)
        if end - start < 0.25:
            self.announce_player(self.t("clip_marker_invalid"))
            return None
        return start, end

    def cancel_clip_preview(self) -> None:
        self.clip_preview_generation += 1

    def preview_marked_clip(self) -> None:
        clip_range = self.marked_clip_range()
        if clip_range is None:
            return
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            self.announce_player(self.t("no_player"))
            return
        self.clip_preview_generation += 1
        preview_generation = self.clip_preview_generation
        player_generation = self.player_generation
        start, end = clip_range
        threading.Thread(
            target=self.preview_marked_clip_worker,
            args=(player_generation, preview_generation, start, end),
            daemon=True,
        ).start()

    def clip_preview_is_current(self, player_generation: int, preview_generation: int) -> bool:
        return (
            player_generation == self.player_generation
            and preview_generation == self.clip_preview_generation
            and self.player_kind == "mpv"
            and self.mpv_process_alive()
        )

    def preview_marked_clip_worker(self, player_generation: int, preview_generation: int, start: float, end: float) -> None:
        try:
            if not self.clip_preview_is_current(player_generation, preview_generation):
                return
            self.mpv_send(["seek", float(start), "absolute+exact"], timeout=0.8)
            self.mpv_set_property("pause", False, timeout=0.8)
            wx.CallAfter(self.start_clip_preview_ui, player_generation, preview_generation, start, end)
            deadline = time.monotonic() + max(1.0, end - start + 2.0)
            while time.monotonic() < deadline:
                if not self.clip_preview_is_current(player_generation, preview_generation):
                    return
                try:
                    position = self.mpv_get_property("time-pos", timeout=0.25)
                except Exception:
                    position = None
                if position is not None and float(position) >= end - 0.03:
                    break
                time.sleep(0.05)
            if not self.clip_preview_is_current(player_generation, preview_generation):
                return
            try:
                self.mpv_send(["seek", float(end), "absolute+exact"], timeout=0.6)
            except Exception:
                pass
            self.mpv_set_property("pause", True, timeout=0.8)
            wx.CallAfter(self.finish_clip_preview_ui, player_generation, preview_generation)
        except Exception:
            if self.clip_preview_is_current(player_generation, preview_generation):
                wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def start_clip_preview_ui(self, player_generation: int, preview_generation: int, start: float, end: float) -> None:
        if not self.clip_preview_is_current(player_generation, preview_generation):
            return
        self.player_ended = False
        self.player_paused = False
        self.update_play_pause_buttons()
        self.announce_player(self.t("clip_preview_started", start=self.format_seconds(start), end=self.format_seconds(end)))

    def finish_clip_preview_ui(self, player_generation: int, preview_generation: int) -> None:
        if not self.clip_preview_is_current(player_generation, preview_generation):
            return
        self.player_paused = True
        self.player_ended = False
        self.update_play_pause_buttons()
        self.announce_player(self.t("clip_preview_finished"))

    def export_marked_clip(self, audio_only: bool = False) -> None:
        clip_range = self.marked_clip_range()
        if clip_range is None:
            return
        start, end = clip_range
        item = dict(self.current_video_item or self.current_video_info or {})
        stream_url = self.current_stream_url
        headers = dict(self.current_stream_headers or {})
        self.announce_player(self.t("clip_export_started"))
        threading.Thread(target=self.export_marked_clip_worker, args=(item, stream_url, headers, start, end, audio_only), daemon=True).start()

    def ffmpeg_executable(self) -> str:
        configured = str(getattr(self.settings, "ffmpeg_location", "") or "").strip()
        if configured:
            configured_path = Path(configured)
            if configured_path.is_dir():
                candidate = configured_path / ("ffmpeg.exe" if os.name == "nt" else "ffmpeg")
                if candidate.exists():
                    return str(candidate)
            elif configured_path.exists():
                return configured
        bundled = self.bundled_path("ffmpeg", "ffmpeg.exe")
        if bundled.exists():
            return str(bundled)
        return shutil.which("ffmpeg") or ""

    def clip_output_extension(self, source: str, item: dict, audio_only: bool = False) -> str:
        if audio_only:
            return f".{self.settings.audio_format}"
        local_path = self.local_media_path_from_input(source)
        if local_path and local_path.suffix:
            return local_path.suffix.lower()
        ext = str(item.get("ext") or "").strip().lower().lstrip(".")
        if ext:
            return f".{ext}"
        kind = str(item.get("kind") or "")
        if kind == "rss_item":
            return ".m4a"
        return ".mp4"

    def export_marked_clip_worker(self, item: dict, stream_url: str, headers: dict, start: float, end: float, audio_only: bool = False) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            input_url = stream_url
            if not input_url:
                input_url, headers, _info = self.resolve_stream_url(str(item.get("url") or ""))
            folder = self.clip_output_folder_for_item(item)
            folder.mkdir(parents=True, exist_ok=True)
            title = str(item.get("title") or Path(input_url).stem or "clip")
            suffix = self.clip_output_extension(input_url, item, audio_only=audio_only)
            output = folder / f"{self.safe_folder_name(title)} - {self.format_seconds(start).replace(':', '-')}-{self.format_seconds(end).replace(':', '-')}{suffix}"
            counter = 2
            while output.exists():
                output = folder / f"{self.safe_folder_name(title)} - {self.format_seconds(start).replace(':', '-')}-{self.format_seconds(end).replace(':', '-')} ({counter}){suffix}"
                counter += 1
            args = [ffmpeg, "-y", "-hide_banner", "-loglevel", "error", "-ss", f"{start:.3f}"]
            header_text = "".join(f"{name}: {value}\r\n" for name, value in headers.items() if value)
            if header_text:
                args.extend(["-headers", header_text])
            args.extend(["-i", input_url, "-t", f"{end - start:.3f}"])
            if audio_only:
                args.extend(self.audio_export_codec_args())
            else:
                args.extend(["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero"])
            args.append(str(output))
            creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
            result = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=creationflags)
            if result.returncode != 0:
                error = (result.stderr or result.stdout or "").strip() or f"FFmpeg exited with code {result.returncode}"
                raise RuntimeError(error[-600:])
            wx.CallAfter(self.announce_player, self.t("clip_export_done", title=output.name))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("clip_export_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def announce_time_async(self) -> None:
        threading.Thread(target=self.announce_time_worker, daemon=True).start()

    def announce_time_worker(self) -> None:
        try:
            elapsed = self.mpv_get_property("time-pos")
            duration = self.mpv_get_property("duration")
            if elapsed is None or duration is None:
                wx.CallAfter(self.announce_player, self.t("timing_unavailable"))
                return
            remaining = max(0, float(duration) - float(elapsed))
            text = self.t(
                "time_announcement",
                elapsed=self.format_seconds(float(elapsed)),
                remaining=self.format_seconds(remaining),
                total=self.format_seconds(float(duration)),
            )
            wx.CallAfter(self.announce_player, text)
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def change_speed_async(self, delta: float) -> None:
        threading.Thread(target=self.change_speed_worker, args=(delta,), daemon=True).start()

    def change_speed_worker(self, delta: float) -> None:
        try:
            current = self.mpv_get_property("speed")
            speed = float(current if current is not None else 1.0)
            speed = self.next_playback_speed(speed, delta)
            self.mpv_set_property("audio-pitch-correction", self.speed_uses_mpv_auto_pitch_correction())
            self.mpv_set_property("speed", speed)
            speed_text = self.format_playback_rate(speed)
            self.current_video_info["speed"] = speed_text
            wx.CallAfter(self.announce_player, self.t("speed_announcement", speed=self.format_rate_for_speech(speed)))
            if self.is_default_rate(speed):
                wx.CallAfter(self.play_default_sound)
            wx.CallAfter(self.update_details_text)
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def change_pitch_async(self, delta: float) -> None:
        threading.Thread(target=self.change_pitch_worker, args=(delta,), daemon=True).start()

    def change_pitch_worker(self, delta: float) -> None:
        pitch = self.next_pitch_value(self.current_pitch_value(), delta)
        speed_delta = delta if self.normalized_pitch_mode() == PITCH_MODE_LINKED_SPEED else None
        for _attempt in range(MPV_PITCH_RETRY_ATTEMPTS):
            try:
                self.apply_pitch_value(pitch, speed_delta=speed_delta)
                wx.CallAfter(self.announce_player, self.t("pitch_announcement", pitch=self.format_rate_for_speech(pitch)))
                if self.is_default_rate(pitch):
                    wx.CallAfter(self.play_default_sound)
                wx.CallAfter(self.update_details_text)
                return
            except Exception:
                if not self.mpv_process_alive():
                    return
                time.sleep(MPV_PITCH_RETRY_DELAY_SECONDS)

    def current_pitch_value(self) -> float:
        stored = self.current_video_info.get("pitch", "1.0")
        try:
            return self.parse_rate_value(stored)
        except (TypeError, ValueError):
            return 1.0

    def apply_pitch_value(self, pitch: float, speed_delta: float | None = None) -> None:
        mode = self.normalized_pitch_mode()
        pitch_text = self.format_playback_rate(pitch)
        if mode == PITCH_MODE_MPV:
            self.clear_rubberband_pitch_filter()
            self.mpv_set_property("audio-pitch-correction", True)
            self.mpv_set_property("pitch", pitch)
        else:
            self.mpv_set_property("audio-pitch-correction", True)
            self.mpv_set_property("pitch", 1.0)
            if self.is_default_rate(pitch):
                self.clear_rubberband_pitch_filter()
            else:
                self.apply_rubberband_pitch_filter(pitch)
            if mode == PITCH_MODE_LINKED_SPEED and speed_delta is not None:
                current_speed = self.mpv_get_property("speed")
                speed = float(current_speed if current_speed is not None else 1.0)
                speed = self.next_playback_speed(speed, speed_delta)
                self.mpv_set_property("speed", speed)
                self.current_video_info["speed"] = self.format_playback_rate(speed)
        self.current_video_info["pitch"] = pitch_text

    def apply_rubberband_pitch_filter(self, pitch: float) -> None:
        if self.rubberband_pitch_filter_active:
            response = self.mpv_request(["af-command", RUBBERBAND_FILTER_LABEL, "set-pitch", f"{pitch:.4f}"])
            if response.get("error") == "success":
                return
            self.rubberband_pitch_filter_active = False
        self.clear_rubberband_pitch_filter()
        response = self.mpv_request(["af", "add", self.rubberband_pitch_filter(pitch)])
        if response.get("error") != "success":
            raise RuntimeError(str(response.get("error") or "rubberband filter not ready"))
        self.rubberband_pitch_filter_active = True

    @staticmethod
    def rubberband_pitch_filter(pitch: float) -> str:
        return f"{RUBBERBAND_FILTER_REF}:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer:pitch-scale={pitch:.4f}"

    def clear_rubberband_pitch_filter(self) -> None:
        try:
            self.mpv_request(["af", "remove", RUBBERBAND_FILTER_REF], timeout=0.8)
        finally:
            self.rubberband_pitch_filter_active = False

    @staticmethod
    def next_pitch_value(current: float, delta: float) -> float:
        return MiscUI.clamp_rate(current + delta, 0.5, 2.0)

    @staticmethod
    def clamp_rate(value: float, minimum: float, maximum: float) -> float:
        return round(min(max(value, minimum), maximum), 2)

    @staticmethod
    def next_step_value(current: float, delta: float, steps: list[float]) -> float:
        if delta < 0:
            for step in reversed(steps):
                if step < current - 0.001:
                    return step
            return steps[0]
        for step in steps:
            if step > current + 0.001:
                return step
        return steps[-1]

    @staticmethod
    def parse_rate_value(value) -> float:
        text = str(value).strip().lower().removesuffix("x").strip()
        return float(text)

    def speed_step_value(self) -> float:
        return self.to_float(str(getattr(self.settings, "speed_step", 0.01)), 0.01, 0.01, 0.25)

    def pitch_step_value(self) -> float:
        return self.to_float(str(getattr(self.settings, "pitch_step", 0.01)), 0.01, 0.01, 0.25)

    def seek_seconds_value(self) -> float:
        return self.to_float(str(getattr(self.settings, "seek_seconds", 5.0)), 5.0, 0.1, 600.0)

    def normalized_pitch_mode(self) -> str:
        mode = str(getattr(self.settings, "pitch_mode", PITCH_MODE_MPV) or PITCH_MODE_MPV)
        return self.normalize_pitch_mode_value(mode)

    def normalized_replaygain_mode(self, value: str | None = None) -> str:
        mode = str(value if value is not None else getattr(self.settings, "replaygain_mode", REPLAYGAIN_MODE_OFF) or REPLAYGAIN_MODE_OFF).strip().lower()
        aliases = {
            "off": REPLAYGAIN_MODE_OFF,
            "none": REPLAYGAIN_MODE_OFF,
            "disabled": REPLAYGAIN_MODE_OFF,
            "track": REPLAYGAIN_MODE_TRACK,
            "song": REPLAYGAIN_MODE_TRACK,
            "album": REPLAYGAIN_MODE_ALBUM,
        }
        return aliases.get(mode, mode if mode in REPLAYGAIN_MODE_OPTIONS else REPLAYGAIN_MODE_OFF)

    def pitch_mode_labels(self) -> list[str]:
        return [
            self.t("pitch_mode_mpv"),
            self.t("pitch_mode_rubberband"),
            self.t("pitch_mode_linked_speed"),
        ]

    def replaygain_mode_labels(self) -> list[str]:
        return [
            self.t("replaygain_off"),
            self.t("replaygain_track"),
            self.t("replaygain_album"),
        ]

    def replaygain_mode_label(self, mode: str | None = None) -> str:
        normalized = self.normalized_replaygain_mode(mode)
        labels = dict(zip(REPLAYGAIN_MODE_OPTIONS, self.replaygain_mode_labels()))
        return labels.get(normalized, self.t("replaygain_off"))

    def audio_normalization_status_label(self) -> str:
        return self.t("audio_normalization_status", mode=self.replaygain_mode_label())

    def cycle_replaygain_mode(self) -> None:
        current = self.normalized_replaygain_mode()
        try:
            index = REPLAYGAIN_MODE_OPTIONS.index(current)
        except ValueError:
            index = 0
        next_mode = REPLAYGAIN_MODE_OPTIONS[(index + 1) % len(REPLAYGAIN_MODE_OPTIONS)]
        self.settings.replaygain_mode = next_mode
        self.save_settings()
        if self.player_kind == "mpv" and self.mpv_process_alive():
            threading.Thread(target=self.apply_replaygain_mode_worker, args=(next_mode,), daemon=True).start()
        self.announce_player(self.t("audio_normalization_changed", mode=self.replaygain_mode_label(next_mode)))
        self.update_player_replaygain_button_label()

    def apply_replaygain_mode_worker(self, mode: str) -> None:
        try:
            self.mpv_set_property("replaygain", self.normalized_replaygain_mode(mode), timeout=0.6)
            self.mpv_set_property("replaygain-clip", "yes", timeout=0.6)
        except Exception:
            wx.CallAfter(self.announce_player, self.t("audio_normalization_restart_needed"))

    def update_player_replaygain_button_label(self) -> None:
        button = getattr(self, "replaygain_button", None)
        if not isinstance(button, wx.Button):
            return
        try:
            label = self.label_with_shortcut(self.audio_normalization_status_label(), "player_replaygain")
            button.SetLabel(label)
            button.SetName(label)
        except RuntimeError:
            pass

    def refresh_interval_labels(self) -> list[str]:
        return [self.refresh_interval_label(option) for option in REFRESH_INTERVAL_OPTIONS]

    def refresh_interval_label(self, value: str) -> str:
        try:
            hours = float(value)
        except (TypeError, ValueError):
            hours = 1.0
        if hours == 0.5:
            return self.t("interval_30_minutes")
        if hours == 1.0:
            return self.t("interval_1_hour")
        label_hours = int(hours) if hours.is_integer() else hours
        return self.t("interval_hours", hours=label_hours)

    @staticmethod
    def normalize_pitch_mode_value(mode: str) -> str:
        normalized = str(mode or "").strip()
        lowered = normalized.lower()
        if normalized in PITCH_MODE_OPTIONS:
            return normalized
        if lowered in {LEGACY_PITCH_MODE_MPV, LEGACY_PITCH_MODE_MPV_LABEL}:
            return PITCH_MODE_MPV
        if lowered == LEGACY_PITCH_MODE_LINKED_SPEED:
            return PITCH_MODE_LINKED_SPEED
        if lowered in {LEGACY_PITCH_MODE_RUBBERBAND, LEGACY_PITCH_MODE_RUBBERBAND_LABEL}:
            return PITCH_MODE_RUBBERBAND
        return PITCH_MODE_MPV

    @staticmethod
    def is_default_rate(value: float) -> bool:
        return abs(value - 1.0) < 0.001

    def play_default_sound(self) -> None:
        try:
            winsound_module = import_module("winsound")
        except ImportError:
            return
        sound_path = self.bundled_path("assets", DEFAULT_REACHED_SOUND)
        try:
            if sound_path.exists():
                winsound_module.PlaySound(str(sound_path), winsound_module.SND_FILENAME | winsound_module.SND_ASYNC)
            else:
                winsound_module.MessageBeep(winsound_module.MB_OK)
        except Exception:
            pass

    @staticmethod
    def youtube_auth_cookie_names() -> set[str]:
        return {
            "sid", "sidcc", "lsid", "osid", "hsid", "ssid",
            "apisid", "sapisid", "login_info", "account_chooser",
            "__secure-osid", "__secure-1psid", "__secure-3psid",
            "__secure-1papisid", "__secure-3papisid",
            "__secure-1psidcc", "__secure-3psidcc",
            "__secure-1psidts", "__secure-3psidts",
        }

    @staticmethod
    def event_key_code(event: wx.KeyEvent) -> int:
        try:
            return int(event.GetKeyCode())
        except Exception:
            return -1

    @staticmethod
    def event_raw_key_code(event: wx.KeyEvent) -> int:
        getter = getattr(event, "GetRawKeyCode", None)
        if not getter:
            return -1
        try:
            return int(getter())
        except Exception:
            return -1

    @staticmethod
    def key_event_codes(event: wx.KeyEvent) -> set[int]:
        codes: set[int] = set()
        for getter_name in ("GetKeyCode", "GetUnicodeKey", "GetRawKeyCode"):
            getter = getattr(event, getter_name, None)
            if not getter:
                continue
            try:
                code = int(getter())
            except Exception:
                continue
            if code not in (-1, 0, wx.WXK_NONE):
                codes.add(code)
        return codes

    @staticmethod
    def is_modifier_only_event(event: wx.KeyEvent) -> bool:
        modifier_codes = {
            getattr(wx, "WXK_CONTROL", -1),
            getattr(wx, "WXK_SHIFT", -1),
            getattr(wx, "WXK_ALT", -1),
            16, 17, 18,
            160, 161, 162, 163, 164, 165,
        }
        codes = MiscUI.key_event_codes(event)
        return bool(codes) and all(code in modifier_codes for code in codes)

    @staticmethod
    def key_event_matches_letter(event: wx.KeyEvent, letter: str) -> bool:
        upper = letter.upper()
        lower = letter.lower()
        wanted = {ord(upper), ord(lower)}
        # Control-character code (e.g. Ctrl+T → 20) only applies when Ctrl is
        # actually held.  Adding it unconditionally caused false positives: for
        # 'T' the control code is 20 which equals VK_CAPITAL (CapsLock), so
        # pressing CapsLock incorrectly fired the player_time shortcut.
        # Similarly Backspace (VK=8) == Ctrl+H code, Tab (VK=9) == Ctrl+I, etc.
        if event.ControlDown():
            wanted.add(ord(upper) - ord("A") + 1)
        for code in MiscUI.key_event_codes(event):
            if code in wanted:
                return True
            if 65 <= code <= 90 and chr(code) == upper:
                return True
            if 97 <= code <= 122 and chr(code) == lower:
                return True
        return False

    @staticmethod
    def is_shift_letter(event: wx.KeyEvent, letter: str) -> bool:
        if not event.ShiftDown():
            return False
        return MiscUI.key_event_matches_letter(event, letter)

    @staticmethod
    def is_ctrl_shift_letter(event: wx.KeyEvent, letter: str) -> bool:
        if not (event.ControlDown() and event.ShiftDown()):
            return False
        return MiscUI.key_event_matches_letter(event, letter)

    @staticmethod
    def safe_folder_name(value: str) -> str:
        cleaned = _RE_UNSAFE_CHARS.sub(" ", str(value or "").strip())
        cleaned = _RE_WHITESPACE.sub(" ", cleaned).strip(" .")
        return cleaned[:150] or "Download"

    def play_favorite(self) -> None:
        item = self.selected_favorite()
        if item:
            self.open_library_item(item, "favorites")

    def copy_plain_text_to_clipboard(self, text: str) -> None:
        if not text:
            return
        if wx.TheClipboard.Open():
            try:
                wx.TheClipboard.SetData(wx.TextDataObject(text))
            finally:
                wx.TheClipboard.Close()

    def refresh_queue_view(self) -> None:
        if not self.in_queue_screen or not hasattr(self, "queue_list"):
            return
        if not self.download_queue and not self.active_downloads:
            self.show_download_queue()
            return
        try:
            selection = self.queue_list.GetSelection()
            self.queue_items = self.download_items_snapshot()
            labels = [self.queue_line(item) for item in self.queue_items]
            self.set_listbox_items(self.queue_list, labels, selection)
        except RuntimeError:
            pass

    def process_queue(self, _event) -> None:
        self.check_activation_signal()
        processed = 0
        max_items_per_tick = 200
        while processed < max_items_per_tick:
            # Only let queue.Empty escape — stop draining when there's nothing left.
            # Each handler is guarded separately so a bug in one handler cannot
            # propagate out of this method and crash the app via wxPython's
            # OnExceptionInMainLoop (which terminates on unhandled exceptions).
            try:
                kind, payload = self.ui_queue.get_nowait()
            except queue.Empty:
                break
            processed += 1
            try:
                if kind == "results":
                    self.show_results(payload)
                elif kind == "status":
                    self.set_status(str(payload))
                elif kind == "announce":
                    self.announce_player(str(payload))
                elif kind == "download_task" and isinstance(payload, dict):
                    task_id = str(payload.pop("task_id", ""))
                    self.update_download_task(task_id, **payload)
                elif kind == "conversion_progress" and isinstance(payload, dict):
                    self.update_conversion_progress_dialog(payload)
                elif kind == "result_metadata" and isinstance(payload, dict):
                    self.apply_result_metadata(payload)
                elif kind == "notify" and isinstance(payload, tuple):
                    title, message = payload
                    self.show_desktop_notification(str(title), str(message), enabled=self.settings.subscription_notifications)
                elif kind == "app_notification" and isinstance(payload, dict):
                    self.add_app_notification(payload)
                elif kind == "subscriptions_changed":
                    self.refresh_subscriptions()
                elif kind == "rss_feeds_changed":
                    if self.rss_items_screen_active and 0 <= self.current_rss_feed_index < len(self.rss_feeds):
                        selection = 0
                        if hasattr(self, "rss_items_list"):
                            try:
                                selection = self.rss_items_list.GetSelection()
                            except RuntimeError:
                                selection = 0
                        self.rss_items = list(self.rss_feeds[self.current_rss_feed_index].get("items") or [])
                        self.refresh_rss_items_list(selection)
                    else:
                        self.refresh_rss_feed_list()
                elif kind == "podcast_results" and isinstance(payload, dict):
                    self.show_podcast_search_results(list(payload.get("results") or []), str(payload.get("query") or ""))
                elif kind == "error":
                    self.message(str(payload), wx.ICON_ERROR)
            except Exception:
                pass

    def set_status(self, text: str) -> None:
        self.status.SetStatusText(text)

