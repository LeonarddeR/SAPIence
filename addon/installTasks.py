# Registers (or unregisters) the bundled x64 and x86 sapience.dll COM servers
# with a single UAC elevation per operation.

import os

import addonHandler
import systemUtils
from gui.message import MessageDialog, ReturnCode


_addon = addonHandler.getCodeAddon()
_ADDON_PATH = _addon.path
_ADDON_DISPLAY_NAME = _addon.manifest["summary"]
_CMD = os.path.join(os.environ["SystemRoot"], "System32", "cmd.exe")
_HELPER_BAT = os.path.join(_ADDON_PATH, "regsvrHelper.bat")
_DLL_X64 = os.path.join(_ADDON_PATH, "dll", "x64", "sapience.dll")
_DLL_X86 = os.path.join(_ADDON_PATH, "dll", "x86", "sapience.dll")


def _verifyBundledFiles() -> None:
	for path in (_HELPER_BAT, _DLL_X64, _DLL_X86):
		if not os.path.isfile(path):
			raise FileNotFoundError(path)


def _askConsent(unregister: bool = False) -> None:
	# Translators: Title of the dialog asking for consent to elevate during add-on install/uninstall.
	title = _("{addonName}: administrator privileges required").format(addonName=_ADDON_DISPLAY_NAME)
	if unregister:
		# Translators: Body of the consent dialog explaining why elevation is needed during uninstall.
		body = _(
			"{addonName} must unregister its SAPI synthesizer from the system-wide registry. "
			"This requires administrator privileges. Do you want to continue?",
		).format(addonName=_ADDON_DISPLAY_NAME)
	else:
		# Translators: Body of the consent dialog explaining why elevation is needed during install.
		body = _(
			"{addonName} must register itself as a SAPI synthesizer in the system-wide registry. "
			"This requires administrator privileges. Do you want to continue?",
		).format(addonName=_ADDON_DISPLAY_NAME)
	# MessageDialog.confirm is thread-safe (marshals to main thread via wxCallOnMain internally).
	if MessageDialog.confirm(message=body, caption=title) != ReturnCode.OK:
		raise RuntimeError("User declined elevation.")


def _runHelper(unregister: bool) -> None:
	# Use cmd.exe /c rather than executing the .bat directly: .bat file association
	# is not guaranteed (could be reassigned to an editor), so we must invoke cmd.exe explicitly.
	args = ["/c", _HELPER_BAT, _DLL_X64, _DLL_X86]
	if unregister:
		args.append("/u")
	rc = systemUtils.execElevated(_CMD, args, wait=True)
	if rc != 0:
		raise RuntimeError(f"regsvrHelper.bat exited with code {rc}.")


def onInstall() -> None:
	_verifyBundledFiles()
	_askConsent(unregister=False)
	_runHelper(unregister=False)


def onUninstall() -> None:
	_verifyBundledFiles()
	_askConsent(unregister=True)
	_runHelper(unregister=True)
