import io
import sys
import types

requests = types.ModuleType('requests')
class Session:
    def request(self, *args, **kwargs):
        assert kwargs['timeout'] == (10, 25)
requests.Session = Session
sys.modules['requests'] = requests

def song():
    return {'videoId': 'dQw4w9WgXcQ', 'title': 'Song', 'duration_seconds': 180,
            'feedbackTokens': {'add': 'private-feedback'}, 'trackingParams': 'private-tracking'}

class Fake:
    instances = 0
    def __init__(self, **kwargs):
        Fake.instances += 1
        assert kwargs['user'] == 'brand-id'
        assert kwargs['auth']['cookie'] == 'private-cookie'
        kwargs['requests_session'].request('GET', 'https://music.youtube.com')
    def get_library_playlists(self, limit):
        return [{'playlistId': 'PL_fixture', 'title': 'List', 'author': [{'name': 'Owner'}], 'trackingParams': 'private-tracking'}]
    def get_liked_songs(self, limit):
        return {'tracks': [song()]}
    def get_playlist(self, playlist, limit):
        assert playlist in ('PL_fixture', 'PL_artist')
        return {'tracks': [song()], 'title': 'List'}
    def get_album(self, album):
        assert album == 'MPRE_album'
        return {'tracks': [song()], 'title': 'Album', 'artists': [{'name': 'Artist', 'id': 'UC_artist'}]}
    def get_artist(self, artist):
        assert artist == 'UC_artist'
        return {'name': 'Artist', 'songs': {'browseId': 'PL_artist'}}
    def search(self, query, filter, limit):
        assert query == 'Artist song' and filter == 'songs'
        return [song()]
    def get_watch_playlist(self, videoId, radio, limit):
        assert videoId == 'dQw4w9WgXcQ'
        assert isinstance(radio, bool)
        return {'tracks': [song()]}

headers = {'cookie': 'private-cookie', 'x-goog-pageid': 'brand-id'}
ids = {'playlist': 'PL_fixture', 'album': 'MPRE_album', 'artist': 'UC_artist',
       'recommendations': 'dQw4w9WgXcQ', 'track': 'dQw4w9WgXcQ'}
for operation in ('account', 'playlists', 'liked', 'playlist', 'album', 'artist', 'search', 'recommendations', 'track'):
    result = namespace['run']({'operation': operation, 'limit': 51, 'headers': headers,
                               'id': ids.get(operation), 'query': 'Artist song'}, factory=Fake)
    assert result['ok'] and result['complete']
    assert 'private-' not in str(result)
    if operation == 'album':
        assert result['items'][0]['album']['id'] == 'MPRE_album'
        assert result['items'][0]['artists'][0]['id'] == 'UC_artist'

clients = {}
before = Fake.instances
for _ in range(3):
    namespace['run']({'operation': 'playlists', 'headers': headers}, factory=Fake, clients=clients)
assert Fake.instances == before + 1

class Failure:
    def __init__(self, **kwargs):
        raise RuntimeError('HTTP 401 private-cookie private-token')
ytmusicapi = types.ModuleType('ytmusicapi')
ytmusicapi.YTMusic = Failure
sys.modules['ytmusicapi'] = ytmusicapi
original_in, original_out = sys.stdin, sys.stdout
output = io.BytesIO()
sys.stdin = types.SimpleNamespace(buffer=io.BytesIO(b'{"operation":"liked"}'))
sys.stdout = types.SimpleNamespace(buffer=output)
namespace['main']()
sys.stdin, sys.stdout = original_in, original_out
assert output.getvalue() == b'{"ok": false, "error": "authentication"}'
print('Read-only bridge operations and credential redaction passed')
