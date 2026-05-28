# Registers (or unregisters) the bundled x64 and x86 sapience.dll COM servers
# with a single UAC elevation per operation.

import os

import addonHandler
import systemUtils
from gui.message import DialogType, MessageDialog, ReturnCode


_addon = addonHandler.getCodeAddon()
_ADDON_PATH = _addon.path
_CMD = os.path.join(os.environ["SystemRoot"], "System32", "cmd.exe")
_HELPER_BAT = os.path.join(_ADDON_PATH, "regsvrHelper.bat")
_DLL_X64 = os.path.join(_ADDON_PATH, "dll", "x64", "sapience.dll")
_DLL_X86 = os.path.join(_ADDON_PATH, "dll", "x86", "sapience.dll")


def _verifyBundledFiles() -> None:
	for path in (_HELPER_BAT, _DLL_X64, _DLL_X86):
		if not os.path.isfile(path):
			raise FileNotFoundError(path)


def _askConsent() -> None:
	# Translators: Title of the dialog asking for consent to elevate during add-on install/uninstall.
	title = _("{addonName}: administrator privileges required").format(addonName=_addon.name)
	# Translators: Body of the consent dialog explaining why elevation is needed.
	body = _(
		"{addonName} must register itself as a SAPI synthesizer in the system-wide registry. "
		"This requires administrator privileges. Do you want to continue?",
	).format(addonName=_addon.name)
	dlg = (
		MessageDialog(
			parent=None,
			message=body,
			title=title,
			dialogType=DialogType.WARNING,
			buttons=None,
		)
		.addYesButton()
		.addNoButton(defaultFocus=True)
	)
	if dlg.ShowModal() != ReturnCode.YES:
		raise RuntimeError("User declined elevation.")


def _runHelper(unregister: bool) -> None:
	# execElevated uses ShellExecuteEx/runas — invoke via cmd.exe so the .bat
	# file association is resolved correctly regardless of NVDA's shell context.
	args = ["/c", _HELPER_BAT, _DLL_X64, _DLL_X86]
	if unregister:
		args.append("/u")
	rc = systemUtils.execElevated(_CMD, args, wait=True)
	if rc != 0:
		raise RuntimeError(f"regsvrHelper.bat exited with code {rc}.")


def onInstall() -> None:
	_verifyBundledFiles()
	_askConsent()
	_runHelper(unregister=False)


def onUninstall() -> None:
	_verifyBundledFiles()
	_askConsent()
	_runHelper(unregister=True)
