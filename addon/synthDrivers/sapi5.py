# Subclass NVDA's built-in SAPI5 driver and hide the SAPIence voice token,
# so NVDA does not list SAPIence as a selectable synthesizer for its own output.

from nvdaBuiltin.synthDrivers.sapi5 import *  # noqa: F401,F403


_SAPIENCE_TOKEN_SUFFIX = "SAPIence"


class SynthDriver(SynthDriver):  # type: ignore[no-redef]  # noqa: F405,F811
	def _getVoiceTokens(self):
		tokens = super()._getVoiceTokens()
		return [tokens[i] for i in range(len(tokens)) if not tokens[i].Id.endswith(_SAPIENCE_TOKEN_SUFFIX)]
