# MediaPipe Selfie Segmentation

Harmony uses the landscape model for on-device camera background replacement.
The original model is from [Google MediaPipe](https://github.com/google-ai-edge/mediapipe/tree/master/mediapipe/modules/selfie_segmentation).
The ONNX export is from
[SelfieBarracuda](https://github.com/keijiro/SelfieBarracuda/tree/47a67596979ed0081c9bb3ddaceea10aa875b800/Packages/jp.keijiro.mediapipe.selfie/ONNX).
Both projects distribute the model under Apache-2.0. See LICENSE. Harmony
removes a redundant output shape attribute from the transposed convolution
to run the graph with RTen. Recreate that change with `prepare.py` and the
Python `onnx` package.

SHA-256 of `selfie_segmentation_landscape.onnx`:
`7b7a71eebd2d161eb19de93a045236de8db465bbc380f1325a191e9c6038b5db`
