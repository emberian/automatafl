"""
Automatafl HTTP game server.

There is a central store of all currently-running games, with the board and
game state. Each game is identified by a UUIDv4. When creating a game, the
client has an option to make the game public or private. Public games will be
listed publically. A player may join any game which they have a URL to, if
there are any empty player slots. Clients are expected to open a websocket
connection to receive notifications of moves and state changes for the games
they are participating in and/or watching.

Sessions store identifiers, also UUIDv4's, which are used to lookup and
track the games a given client is participating in.

The server keeps a log of committed transactions (sets of moves) for a
not-yet-implemented replay feature.
"""

from contextlib import contextmanager
import os
import uuid

from flask.json import jsonify
from flask import Flask, session, abort
from flask_socketio import SocketIO, join_room, leave_room, send, emit
import model

class FEPleb(model.Plebeian):
    def __init__(self, id, name, uuid):
        self.id = id
        self.name = name
        self.uuid = uuid

class FEGame(object):
    def __init__(self, name):
        self.name = name
        self.private = False
        self.max_plebid = 0
        self.sess_plebs = {}
        self.uuid_plebs = {}
        self.uuid = uuid.uuid4()
        self.committed_moves = []
        self.setup = None
        self.mgame = None

    def add_pleb(self, name, sessid):
        pleb = FEPleb(self.max_plebid, name, uuid.uuid4())
        self.sess_plebs[sessid] = pleb
        self.uuid_plebs[pleb.uuid] = pleb
        self.max_plebid += 1
        return self.plebs[-1]

    def create_game_model(self):
        self.mgame = model.Game(plebs=self.plebs, setup=self.setup)

    def pleb_uuids(self):
        return [pleb.uuid for pleb in self.plebs]

    def pleb_from_sess(self, sess):
        return self.sess_plebs.get(sess)

    def pleb_from_uuid(self, uuid):
        return self.uuid_plebs.get(uuid)

    def serialize(self):
        return {
            'board': None if self.mgame is None else self.mgame.Serialize(),
            'name': self.name,
            'players': len(self.sess_plebs),
        }

    # TODO: Setup configuration (chosing initial board etc)

def is_coord(thing):
    # Coordinates are a [y, x] pair; JSON should deserialize them as a list.
    return isinstance(thing, list) and len(thing) == 2 and isinstance(thing[0], int)

# Map from UUID to dict.
client_session_states = {}

# Map from UUID to FEGame
current_games = {}

app = Flask(__name__)
app.secret_key = os.urandom(32)


socketio = SocketIO(app)

def sess_uuid():
    if "sess" not in session:
        session["sess"] = uuid.uuid4()
    return session["sess"]

def client_sess_state():
    uid = sess_uuid()

    if uid not in client_session_states:
        d = {}
        client_session_states[uid] = d
        d["in_games"] = []
    return client_session_states.get(uid)

@app.route("/game", methods=["GET"])
def list_games():
    return jsonify([
        feg.serialize()
        for feg in current_games.values()
        if not feg.private or feg.pleb_from_sess(sess_uuid())
    ])

@app.route("/game/<uuid:gameid>", methods=["GET"])
def get_game(gameid):
    feg = current_games.get(gameid, None)
    if feg is None:
        abort(404)

    return jsonify({"status": "ok", "game": feg.serialize()})

@app.route("/game/<uuid:gameid>/join", methods=["POST"])
def join_game(gameid):
    feg = current_games.get(gameid, None)
    if feg is None:
        abort(404)
    if hasattr(feg, "mgame"):
        abort(403)
    j = request.get_json()
    if "name" not in j:
        abort(400)

    feg.add_pleb(j["name"], sess_uuid())
    return jsonify({"status": "ok"})

@app.route("/game", methods=["POST"])
def make_game():
    j = request.get_json()
    if "name" not in j:
        abort(400)

    feg = FEGame(j["name"])
    current_games[feg.uuid] = feg
    return jsonify({"status": "ok", "uuid": feg.uuid})

@socketio.on("subscribe_to_game")
def subscribe_to_game(msg):
    if "reqid" not in msg:
        return {"status": "error", "reqid": 0, "error": "NO_REQID"}
    elif "sessid" not in msg:
        return {"status": "error", "reqid": msg["reqid"], "error": "NO_SESSID"}
    elif "game_id" not in msg:
        return {"status": "error", "reqid": msg["reqid"], "error": "NEED_GAME_ID"}
    elif msg["game_id"] not in current_games:
        return {"status": "error", "reqid": msg["reqid"], "error": "GAME_NOT_EXIST"}
    elif msg["game_id"] not in client_sess_state()["in_games"]:
        return {"status": "error", "reqid": msg["reqid"], "error": "NOT_IN_GAME"}
    else:
        join_room(msg["game_id"])
        return {"status": "ok", "reqid": msg["reqid"]}

@socketio.on("unsubscribe_from_game")
def unsubscribe_from_game(msg):
    if "reqid" not in msg:
        return {"status": "error", "reqid": 0, "error": "NO_REQID"}
    elif "game_id" not in msg:
        return {"status": "error", "reqid": msg["reqid"], "error": "NEED_GAME_ID"}
    else:
        leave_room(msg["game_id"])
        return {"status": "ok", "reqid": msg["reqid"]}

@socketio.on("submit_move")
def submit_move(msg):
    s = client_sess_state()

    if "reqid" not in msg:
        return {"status": "error", "reqid": 0, "error": "NO_REQID"}
    elif "game_id" not in msg:
        return {"status": "error", "reqid": msg["reqid"], "error": "NEED_GAME_ID"}
    elif msg["game_id"] not in s["in_games"]:
        return {"status": "error", "reqid": msg["reqid"], "error": "NOT_IN_GAME"}
    elif msg["game_id"] not in current_games:
        return {"status": "error", "reqid": msg["reqid"], "error": "GAME_NOT_EXIST"}
    elif not is_coord(msg.get("from")) or not is_coord(msg.get("to")):
        return {"status": "error", "reqid": msg["reqid"], "error": "NEED_COORDS"}
    else:
        feg = current_games[msg["game_id"]]
        plebid = feg.pleb_from_sess(sess_uuid())
        iev = model.Move(plebid, msg["from"], msg["to"])
        oev = feg.mgame.Handle(iev)
        if len(feg.mgame.pending_moves) == len(feg.mgame.plebeians):
            conflicts = feg.mgame.Resolve()
            emit("resolved", {"status": "ok", "event": [c.serialize() for c in
                conflicts]}, broadcast=True, room=feg.uuid)
        return {"status": "ok", "reqid": msg["reqid"], "event": oev.serialize()}

@app.route("/")
def index():
    return app.send_static_file("index.html")

if __name__ == "__main__":
    socketio.run(app)
