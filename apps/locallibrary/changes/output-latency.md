### Fixed

- The player's playhead follows what's heard. It counted what had been handed to the output,
  which plays it a buffer and the device's latency later (about 100 ms through Bluetooth), so
  the playhead ran ahead of the sound in steps of a buffer. A loop made by ear with quantize off
  started that much after the bar and was drawn there. The output now reports when each buffer
  will be heard, and the playhead, the cue flashes, the beat count and new loops go by that. On
  pause the playhead stops where the sound does.
