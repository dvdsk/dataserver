var wsUri = 'wss://deviousd.duckdns.org:8080/ws/';
var plotmeta = new Map();
var output;
var subbed = new Map();

var lines;
var id_map = new Map();

var layout = {
	xaxis: {title: 'time (s)'},
	yaxis: {title: 'humidity',}
};


function init(){
  output = document.getElementById("output");

  websocket = new WebSocket(wsUri);
  websocket.binaryType = 'arraybuffer';
  websocket.onopen = function(evt) { onOpen(evt) };
  websocket.onclose = function(evt) { onClose(evt) };
  websocket.onerror = function(evt) { onError(evt) };
}

function onOpen(evt){
  writeToScreen("CONNECTED");

  //parse the form
  var selected = document.forms["choose lines"];
  for (var i = 0; i < selected.length; i++) {
    if (selected[i].checked === true) {
      var input = selected[i].value;
      let [set_id_str, field_id] = input.split(",");
      let set_id = Number(set_id_str);
      if (subbed.has(set_id)) {
        field_list = subbed.get(set_id);
        field_list.push(field_id);
      } else {
        subbed.set(set_id, [field_id]);
      }
    }
  }

  //generate and send subscribe string
  for (const [set,fields] of subbed.entries()){
    var s = "/select_uncompressed ";
    s=s+set;
    for (var i = 0; i < fields.length; i++ ) {
      s=s+" "+fields[i];
    }
    console.log(s);
    doSend(s);
  }

  websocket.onmessage = function(evt) { gotMeta(evt) };
  doSend("/meta");
}

function gotMeta(evt){
  showMessage(evt);

  var id_info;
  ({id_info, lines} = JSON.parse(evt.data));

  var i = 0;
  while (i < id_info.length) {
    var set_id =  id_info[i].dataset_id;
    field_list = [];
    do {
      field_list.push({field_id: id_info[i].field_id, trace_numb: i});
      lines[i].x = new Array(); lines[i].y = new Array();
      i++;
    } while(i < id_info.length && id_info[i].dataset_id == set_id)
    id_map.set(set_id, field_list);
  }

  websocket.onmessage = function(evt) { gotInitTimestamps(evt) };
  doSend("/data");
}

function gotInitTimestamps(evt){
  websocket.onmessage = function(evt) { gotInitData(evt) };
  var len = evt.data.byteLength;
  //console.log("len timestamps: "); console.log(len);
  var timestamps = new Float64Array(evt.data, 0, len/8);
  //console.log("timestamps: "); console.log(timestamps);
  for (var i = 0; i < lines.length; i++) {
    // Iterate over numeric indexes from 0 to 5, as everyone expects.
    lines[i].x = Array.from(timestamps);
  }
}

function gotInitData(evt){
  websocket.onmessage = function(evt) { gotUpdate(evt) };
  var len = evt.data.byteLength;
  var data = new Float32Array(evt.data, 0, len/4);
  //console.log("len data: "); console.log(data.length);
  //console.log(lines.length);
  //console.log(data);
  for (var i=0; i < data.length; i+=lines.length){
    //console.log("test");
    for (var j=0; j < lines.length; j++){
      //console.log(j);
      //console.log(i+j);
      lines[j].y.push(data[i+j]);
    }
  }
  console.log(lines);
  Plotly.newPlot("plot", lines, layout, {responsive: true});
  doSend("/sub");
}

function gotUpdate(evt){
  data = new DataView(evt.data);
  setid = data.getInt16();
  timestamp = data.getFloat64(2, true);

  var fields_to_lines = id_map.get(setid);
  //TODO rethink metadata ordening (use nested list)

  var x_update = [];
  var y_update = [];
  var updated_traces = [];
  //console.log(setid);
  //console.log(id_map);
  var len = fields_to_lines.length;
  for (var i=0; i < len; i++) {
    var trace_numb = fields_to_lines[i].trace_numb;
    updated_traces.push(trace_numb);
    x_update.push([timestamp]);
    y_update.push([data.getFloat32(4*i+10, true)]);
  }
  Plotly.extendTraces("plot", {x: x_update, y: y_update}, updated_traces);

  writeToScreen("Got Update");

}




function doSend(message){
  writeToScreen("SENT: " + message);
  websocket.send(message);
}

function onClose(evt){
  writeToScreen("DISCONNECTED");
}

function showMessage(evt){
  writeToScreen('<span style="color: blue;">RESPONSE: ' + evt.data+'</span>');
}

function onError(evt){
  writeToScreen('<span style="color: red;">ERROR:</span> ' + evt.data);
}

function writeToScreen(message){
  var pre = document.createElement("p");
  pre.style.wordWrap = "break-word";
  pre.innerHTML = message;
  output.appendChild(pre);
}

//window.addEventListener("load", init, false);
