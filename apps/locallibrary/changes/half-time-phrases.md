### Fixed

- Phrases rekordbox counted at half tempo run the whole track. On some fast tracks (seen from
  141 BPM up) rekordbox numbers the phrases' beats at half the grid's tempo and says so nowhere, so
  the phrase strip stopped halfway through the track and the phrase readout's bars were halved.
  The player, the mix view, mix reports and exports, and `track show`'s phrase bars now place them
  on the grid at half tempo when they end halfway through it.
