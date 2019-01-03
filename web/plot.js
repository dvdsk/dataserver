var wsUri = 'wss://deviousd.duckdns.org:8080/ws/';
var plotmeta = new Map();
var output;
var subbed = new Map();

var initdata_fields_to_lines;
var initdata_is_last_chunk;

var lines;
//maps set_id"s to indexes for lines
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
    console.log(set_id);
    console.log(id_map);
  }
  console.log("gotMeta");
  websocket.onmessage = function(evt) { gotInitDataInfo(evt) };
  doSend("/data");
}

function gotInitDataInfo(evt){
  var data = new DataView(evt.data);
  initdata_is_last_chunk = data.getInt8(0, true);
  var setid = data.getInt16(1, true);
  initdata_fields_to_lines = id_map.get(setid);
  console.log("initdata field to lines");
  websocket.onmessage = function(evt) { gotInitTimestamps(evt) };
}

function gotInitTimestamps(evt){
  websocket.onmessage = function(evt) { gotInitData(evt) };
  var len = evt.data.byteLength;
  var floatarr = new Float64Array(evt.data, 0, len/8);
  var timestamps = Array.from(floatarr);
  var dates = timestamps.map(x => new Date(x*1000));
  //look up which traces x-axis to append to
  for (var i = 0; i < initdata_fields_to_lines.length; i++) {
    var trace_numb = initdata_fields_to_lines[i].trace_numb;
    lines[trace_numb].x.push(dates);
  }
  console.log(timestamps);
}

function gotInitData(evt){
  var len = evt.data.byteLength;
  var data = new Float32Array(evt.data, 0, len/4);
  for (var i=0; i < data.length; i+=lines.length){
    for (var j=0; j < initdata_fields_to_lines.length; j++){
      var trace_numb = initdata_fields_to_lines[j].trace_numb;
      lines[trace_numb].y.push(data[i+j]);
    }
  }
  console.log(lines);
  Plotly.newPlot("plot", lines, layout, {responsive: true});

  if (initdata_is_last_chunk != 0) {
    websocket.onmessage = function(evt) { gotUpdate(evt) };
    doSend("/sub");
  }
}

function gotUpdate(evt){
  data = new DataView(evt.data);
  setid = data.getInt16(0, true);
  timestamp = data.getFloat64(2, true);

  console.log(setid);
  console.log(id_map);
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
    x_update.push([new Date(timestamp*1000)]);
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
