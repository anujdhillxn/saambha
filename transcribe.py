import os

_model = None

def get_model():
    global _model
    if _model is None:
        from faster_whisper import WhisperModel
        model_size = os.environ.get("WHISPER_MODEL", "base")
        _model = WhisperModel(model_size, device="cpu", compute_type="int8")
    return _model


def transcribe(audio_path: str) -> str:
    model = get_model()
    segments, _ = model.transcribe(audio_path, language="en")
    return " ".join(seg.text for seg in segments).strip()
