-- Two tracks for the WAL fixture: the first is checkpointed into the database file, the second
-- (with a playlist entry) only exists in the WAL. See make.sh.
INSERT INTO artist VALUES(1,'Alpha',NULL);
INSERT INTO "key" VALUES(1,'Am');
INSERT INTO content (content_id, title, subtitle, bpmx100, length, artist_id_artist, artist_id_lyricist, key_id, color_id, djComment, releaseDate, dateAdded, path, fileName, fileSize, fileType, analysisDataFilePath, analysedBits)
  VALUES(1,'One','',12800,300,1,0,1,0,'','','2026-10-01','/Contents/Alpha/One.mp3','One.mp3',1000,1,'/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT',105);
INSERT INTO playlist VALUES(1,0,'Set',NULL,0,0);
INSERT INTO playlist_content VALUES(1,1,1);
UPDATE property SET numberOfContents = 1;
PRAGMA wal_checkpoint(TRUNCATE);
PRAGMA wal_autocheckpoint = 0;
INSERT INTO artist VALUES(2,'Beta',NULL);
INSERT INTO content (content_id, title, subtitle, bpmx100, length, artist_id_artist, artist_id_lyricist, key_id, color_id, djComment, releaseDate, dateAdded, path, fileName, fileSize, fileType, analysisDataFilePath, analysedBits)
  VALUES(2,'Two','',12400,240,2,0,0,3,'','','2026-10-02','/Contents/Beta/Two.mp3','Two.mp3',2000,1,'/PIONEER/USBANLZ/P002/00000002/ANLZ0000.DAT',105);
INSERT INTO playlist_content VALUES(1,2,2);
UPDATE property SET numberOfContents = 2;
