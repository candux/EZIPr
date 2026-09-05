# TODO

## Format compatibility

- Decode and encode the AGIF container produced from GIF animations. Treat it
  as a distinct animation format rather than another spelling of eZIP-A.
- Decode and encode indexed-palette static resources and animations. Confirm
  palette length encoding, byte order, transparency, and index storage before
  adding the public pixel format.
- Decode and encode grayscale and grayscale-alpha eZIP and PIXEL resources.
- Establish the output constraints for eZip hardware generations and SF32
  families. Add a target profile only after the differences that affect asset
  compatibility are confirmed.

## RGB565 perceptual validation

- Compare the shared Bayer threshold against phase-shifted or luma-aware
  alternatives on neutral gray ramps using target hardware. Preserve the
  balanced mode's reconstruction-level fixed point in any alternative.
