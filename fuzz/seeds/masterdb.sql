-- The masterdb target's seed, seeds/masterdb/master.db (made as the Fuzzing section of
-- apps/locallibrary/CONTRIBUTING.md says): the tables locallibrary reads from rekordbox's
-- master.db, with pyrekordbox's column names (less the ones nothing reads), and a few rows of
-- each. Small pages keep the input small.
PRAGMA page_size = 512;
CREATE TABLE djmdArtist (ID VARCHAR(255) PRIMARY KEY, Name VARCHAR(255), rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdAlbum (ID VARCHAR(255) PRIMARY KEY, Name VARCHAR(255), AlbumArtistID VARCHAR(255), rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdGenre (ID VARCHAR(255) PRIMARY KEY, Name VARCHAR(255), rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdLabel (ID VARCHAR(255) PRIMARY KEY, Name VARCHAR(255), rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdKey (ID VARCHAR(255) PRIMARY KEY, ScaleName VARCHAR(255), Seq INTEGER, rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdContent (ID VARCHAR(255) PRIMARY KEY, FolderPath VARCHAR(255), Title VARCHAR(255), ArtistID VARCHAR(255),
  AlbumID VARCHAR(255), GenreID VARCHAR(255), BPM INTEGER, Length INTEGER, TrackNo INTEGER, BitRate INTEGER, BitDepth INTEGER,
  Commnt TEXT, Rating INTEGER, ReleaseYear INTEGER, RemixerID VARCHAR(255), LabelID VARCHAR(255), OrgArtistID VARCHAR(255),
  KeyID VARCHAR(255), StockDate VARCHAR(255), ColorID VARCHAR(255), DJPlayCount VARCHAR(255), ImagePath VARCHAR(255),
  AnalysisDataPath VARCHAR(255), FileSize INTEGER, DiscNo INTEGER, ComposerID VARCHAR(255), Subtitle VARCHAR(255),
  SampleRate INTEGER, ReleaseDate VARCHAR(255), DateCreated VARCHAR(255), ISRC VARCHAR(255),
  rb_local_deleted TINYINT(1) DEFAULT 0, created_at TEXT);
CREATE TABLE djmdCue (ID VARCHAR(255) PRIMARY KEY, ContentID VARCHAR(255), InMsec INTEGER, OutMsec INTEGER, Kind INTEGER,
  ColorTableIndex INTEGER, Comment VARCHAR(255), rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdPlaylist (ID VARCHAR(255) PRIMARY KEY, Seq INTEGER, Name VARCHAR(255), Attribute INTEGER, ParentID VARCHAR(255),
  SmartList TEXT, rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdSongPlaylist (ID VARCHAR(255) PRIMARY KEY, PlaylistID VARCHAR(255), ContentID VARCHAR(255), TrackNo INTEGER,
  rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdHistory (ID VARCHAR(255) PRIMARY KEY, Seq INTEGER, Name VARCHAR(255), Attribute INTEGER, ParentID VARCHAR(255),
  rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdSongHistory (ID VARCHAR(255) PRIMARY KEY, HistoryID VARCHAR(255), ContentID VARCHAR(255), TrackNo INTEGER,
  rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdMyTag (ID VARCHAR(255) PRIMARY KEY, Seq INTEGER, Name VARCHAR(255), Attribute INTEGER, ParentID VARCHAR(255),
  rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdSongMyTag (ID VARCHAR(255) PRIMARY KEY, MyTagID VARCHAR(255), ContentID VARCHAR(255), TrackNo INTEGER,
  rb_local_deleted TINYINT(1) DEFAULT 0);
CREATE TABLE djmdProperty (DBID VARCHAR(255) PRIMARY KEY, DBVersion VARCHAR(255));
CREATE TABLE djmdCloudProperty (ID VARCHAR(255) PRIMARY KEY, Reserved1 TEXT, Reserved2 TEXT);
CREATE TABLE contentFile (ID VARCHAR(255) PRIMARY KEY, ContentID VARCHAR(255), Path VARCHAR(255), Size INTEGER);

INSERT INTO djmdProperty VALUES ('1234567890', '7000');
INSERT INTO djmdArtist (ID, Name) VALUES ('1', 'Daft Punk'), ('2', 'Floorplan');
INSERT INTO djmdAlbum (ID, Name, AlbumArtistID) VALUES ('1', 'Discovery', '1');
INSERT INTO djmdGenre (ID, Name) VALUES ('1', 'House');
INSERT INTO djmdLabel (ID, Name) VALUES ('1', 'Virgin');
INSERT INTO djmdKey (ID, ScaleName, Seq) VALUES ('1', 'Am', 1);
INSERT INTO djmdContent (ID, FolderPath, Title, ArtistID, AlbumID, GenreID, LabelID, KeyID, BPM, Length, Rating, ColorID,
  DJPlayCount, ImagePath, AnalysisDataPath, StockDate, ReleaseDate, ISRC) VALUES
  ('84935471', '/Users/dj/Music/One More Time.mp3', 'One More Time', '1', '1', '1', '1', '1', 12280, 320, 5, '2', '7',
   '/PIONEER/Artwork/00001/a1.jpg', '/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT', '2024-03-01', '2000-11-30', 'GBDUW0000059'),
  ('205356547', '/contents_2354715273/Floorplan/ngo.mp3', 'Never Grow Old', '2', NULL, NULL, NULL, NULL, 12800, 412, 0, '0',
   '0', NULL, NULL, NULL, NULL, NULL);
INSERT INTO djmdContent (ID, Title, DateCreated, rb_local_deleted) VALUES ('3', 'Deleted', '2023-12-24', 1);
INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, SmartList) VALUES
  ('10', 1, 'Shows', 1, 'root', NULL), ('11', 1, 'Friday', 0, '10', NULL),
  ('12', 2, 'Recent', 4, 'root', '<NODE Id="-1" LogicalOperator="2" AutomaticUpdate="1"><CONDITION PropertyName="genre" Operator="8" ValueUnit="" ValueLeft="House" ValueRight=""/><CONDITION PropertyName="myTag" Operator="8" ValueUnit="" ValueLeft="101" ValueRight=""/><CONDITION PropertyName="stockDate" Operator="6" ValueUnit="month" ValueLeft="3" ValueRight=""/></NODE>');
INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo) VALUES
  ('a', '11', '84935471', 2), ('b', '11', '205356547', 1), ('c', '10', '3', 1);
INSERT INTO djmdHistory (ID, Seq, Name, Attribute, ParentID) VALUES ('20', 1, 'HISTORY 2024-03-01', 0, 'root');
INSERT INTO djmdSongHistory (ID, HistoryID, ContentID, TrackNo) VALUES ('h', '20', '84935471', 1);
INSERT INTO djmdCue (ID, ContentID, InMsec, OutMsec, Kind, ColorTableIndex, Comment) VALUES
  ('c1', '84935471', 1000, -1, 1, 1, 'Drop'), ('c2', '84935471', 2000, 4000, 4, 0, 'Loop'), ('c3', '205356547', 500, -1, 5, 3, '');
INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID) VALUES
  ('100', 0, 'Genre', 1, 'root'), ('101', 0, 'Deep', 0, '100'), ('200', 1, 'Situation', 1, 'root'), ('201', 0, 'Peak', 0, '200');
INSERT INTO djmdSongMyTag (ID, MyTagID, ContentID, TrackNo) VALUES ('t1', '101', '84935471', 1), ('t2', '201', '205356547', 1);
INSERT INTO djmdCloudProperty VALUES ('1', 'SyncService', 'Dropbox');
INSERT INTO contentFile VALUES ('1', '205356547', '/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT', 1234);
