class WhisperTranscriber:
    """Speech to text with faster-whisper; the model loads on first use."""

    def __init__(self, model_size: str):
        self.model_size = model_size
        self._model = None

    def transcribe(self, audio_path: str) -> str:
        if self._model is None:
            from faster_whisper import WhisperModel
            self._model = WhisperModel(self.model_size, device="cpu", compute_type="int8")
        segments, _ = self._model.transcribe(audio_path, language="en")
        return " ".join(seg.text for seg in segments).strip()
