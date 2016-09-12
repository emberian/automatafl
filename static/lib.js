function Pleb(id, name) {
	this.id = id;
	this.name = name;
}

function Coord(x, y) {
	this.x = x;
	this.y = y;
}

Coord.fromElement = function(el) {
	return new Coord(parseInt(el.dataset.x), parseInt(el.dataset.y));
}

Coord.fromJSON = function(js) {
	var res = JSON.parse(js);
	return new Coord(res[1], res[0]);
}

Coord.prototype.toJSON = function() {
	return JSON.stringify([this.y, this.x]);
}

Coord.prototype.toString = function() {
	return "<" + this.x + "," + this.y + ">";
}

function ClientState(idme, plebs, board) {
	if(plebs == undefined) { plebs = {}; }
	if(board == undefined) {
		board = [];
		for(var colidx = 0; colidx < 11; colidx++) {
			var row = [];
			board.push(row);
			for(var rowidx = 0; rowidx < 11; rowidx++) {
				piece = {
					"occupant": 0,
					"conflict": false,
				};
				row.push(piece);
			}
		}
	}
	this.socket = io('http://' + document.domain + ':' + location.port + '/');
	this.socket.on('oevent', this._onoevent);
	this.outstanding = {};
	this.nextSerial = 0;
	this.plebs = plebs;
	this.idme = idme;
	this.board = board;
}

Object.defineProperty(ClientState, 'me', {
	"get": function() {
		return this.plebs[this.idme];
	},
});

ClientState.prototype._onoevent = function(msg) {
	var ev = this.outstanding[

ClientState.prototype._ondragstart = function(ev) {
	ev.dataTransfer.setData('src_bpos', Coord.fromElement(ev.target).toJSON());
}

ClientState.prototype._ondragover = function(ev) {
	ev.preventDefault();
}

ClientState.prototype._ondrop = function(ev) {
	var co = Coord.fromJSON(ev.dataTransfer.getData('src_bpos'));
	this.submitMove

ClientState.prototype.render_board = function() {
	var root = document.createElement('table');
	root.classList.add('board');
	for(var colidx = 0; colidx < this.bdata.length; colidx++) {
		var row = this.bdata[col];
		var trow = document.createElement('tr');
		root.appendChild(trow);
		for(var rowidx = 0; rowidx < row.length; rowidx++) {
			var piece = row[rowidx];
			var tpiece = document.createElement('td');
			trow.appendChild(tpiece);
			tpiece.classList.add("pc_" + piece.occupant);
			if(piece.conflict) {
				tpiece.classList.add("pf_conflict");
			}
			if(piece.goal) {
				tpiece.classList.add("pf_goal");
			}
		}
	}
	return root;
}

