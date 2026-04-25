# lenzu-models-dbnet

AGPL-3.0 DBNet text-detection model, packaged separately from the MIT-licensed
`lenzu` and `lenzu-hud` to keep the main packages copyleft-free.

## Build

The model file is staged from `assets/stabrise-text_detection_dbnet_ml_v02_model.onnx`
at build time. Run from the repo root:

```sh
make deb-dbnet
```

This produces `target/debian/lenzu-models-dbnet_0.2.0_all.deb`.

## License

This package contains the DBNet ONNX model from
[StabRise/text_detection_dbnet_ml_v0.2](https://huggingface.co/StabRise/text_detection_dbnet_ml_v0.2),
licensed under **AGPL-3.0**.  See `copyright` for details.

The `lenzu` MIT package treats this as an *optional* dependency
(`Recommends:`, not `Depends:`).  Without it, lenzu falls back to full-image OCR.
