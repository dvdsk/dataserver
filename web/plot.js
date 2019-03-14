var loc = window.location;
var wsUri = "wss://"+loc.hostname+":"+loc.port+"/ws/";

var plotmeta = new Map();
var output;
var subbed = new Map();

var initdata_fields_to_lines;
var initdata_is_last_chunk;

var package_size;
var lines = [];
//maps set_id"s to indexes for traces, and position in traces
var id_map = new Map();

var layout = {
	xaxis: {
	  type: 'date',
	  title: 'time (s)'},
	yaxis: {title: 'humidity',}
};

//TODO extend id_map to keep track of position is typedArray

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
  //TODO change plot button to "update"
  var selected = document.forms["choose lines"];
  for (var i = 0; i < selected.length; i++) {//TODO selected.for ... of ... (look up)
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

  var start_timestamp = 0;//placeholder
  var stop_timestamp = 0;
  //generate and send subscribe string
  for (const [set,fields] of subbed.entries()){
    var s = "/select_uncompressed "
    +start_timestamp+" "
    +stop_timestamp+" "
    +set+" ";

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
  var sets_meta = JSON.parse(evt.data);//add package info (total data size, package size)
  //console.log(sets_meta);

  //for every dataset
  for (var i=0; i < sets_meta.length; i+=1){
    ({field_ids, traces_meta, n_lines, dataset_id} = sets_meta[i]);

    //console.log("sets_meta");
    //console.log(sets_meta[i]);

    var old_len = lines.length;
    var field_list = [];
    //append all the metadata to the lines
    //console.log("lines before");
    //console.log(lines);
    //console.log("traces_meta");
    //console.log(traces_meta);
    lines = lines.concat(traces_meta);
    //console.log("lines");
    //console.log(lines);

    //create an x-array for all a dataset
    var shared_x = new Float64Array(n_lines);
    for (var i= 0; i<field_ids.length; i++) {
      //for each new line/field/y-value link the x-array and allocate a y-array
      field_list.push({field_id: field_ids[i], trace_numb: old_len+i});
      lines[old_len+i].x = shared_x;
      lines[old_len+i].y = new Float32Array(n_lines);
    }
    //console.log("n_lines");
    //console.log(n_lines);

    //console.log("lines allocated");
    //console.log(lines);

    //debugger;
    id_map.set(dataset_id, [field_list, 0]);
    //console.log("inserting field list with set_id: ");
    //console.log(dataset_id);
    //console.log(id_map);
  }
  //console.log("gotMeta");
  websocket.onmessage = function(evt) { gotDataChunk(evt) };
  doSend("/RTC");
}

function setTimestamps(data, numb_of_elements, fields_to_lines, pos){
  //console.log("raw data");
  //console.log(data);

  var floatarr = new Float64Array(data, 8, numb_of_elements);
  var timestamps = floatarr.map(x => x*1000); //from seconds to milliseconds
  //no need to set for all x-axises as they are linked
  //memcpy equivalent of memcpy(trace+pos, timestamps, len(timestamps));
  trace_numb = fields_to_lines[0].trace_numb;
  //console.log("fields to lines[0]"); console.log(fields_to_lines[0]);
  //console.log("fields to lines"); console.log(fields_to_lines);
  //console.log("lines"); console.log(lines);
  //console.log("trace_numb"); console.log(trace_numb);
  // Copy the new timestamps into the array starting at index pos
  console.log("timestamps"); console.log(timestamps);
  lines[trace_numb].x.set(timestamps, pos);
  console.log("lines"); console.log(lines);
}

function setData(data, numb_of_elements, fields_to_lines, pos){
  var nTraces_in_set = fields_to_lines.length;
  var data = new Float32Array(data, 8+numb_of_elements*8, numb_of_elements);
  //console.log("data:"); console.log(data);
  //console.log("pos"); console.log(pos);

  for (var i=0; i < numb_of_elements; i+=1){
    for (var j=0; j < nTraces_in_set; j++){
      var trace_numb = fields_to_lines[j].trace_numb;
      lines[trace_numb].y[pos+i] = data[i+j];
      //console.log("trace_numb"); console.log(trace_numb);
      //console.log("pos+i"); console.log(pos+i);
    }
  }
}

//allocate data for all chunks
function gotDataChunk(evt){ //FIXME only works for one dataset
  //console.log("evt");
  //console.log(evt);

  var data = new DataView(evt.data);
  //check for server signal that all data has been recieved, or an error has
  //occured
  if (data.getInt16(2, true) == 1) { //check if this was the last package
    //console.log("got last data chunk, creating plot");
    Plotly.newPlot("plot", lines, layout, {responsive: true});
    websocket.onmessage = function(evt) { gotUpdate(evt) };
    doSend("/sub");
    return;
  };

  var chunknumb = data.getInt16(0, true);
  var setid = data.getInt16(2, true);
  var [fields_to_lines, pos] = id_map.get(setid);
  //console.log("fields_to_lines:"); console.log(fields_to_lines);
  //console.log("id_map:"); console.log(id_map);
  //console.log("setid:"); console.log(setid);

  var numb_of_elements = (evt.data.byteLength-8)/(4*(fields_to_lines.length)+8);
  //console.log("numb_of_elements"); console.log(numb_of_elements);
  //console.log("evt.data"); console.log(evt.data);
  //console.log(fields_to_lines.length);

  //FIXME NEEDS TO MOVE TO META DATA, NO TIME TO PRE ALLOC CURRENTLY
  //next package arrives and is handled before this is done

  //packages can arrive out of order, needed changes:
  //
  //--determine pos not from last (vector/write style) but chunknumber and known chunk sizes
  //--add check if allocation is finished before continueing (use global bool flag for this)

  //console.log("numb_of_elements"); console.log(numb_of_elements);
  //console.log("package_size"); console.log(package_size);

  //console.log(numb_of_elements);
  setTimestamps(evt.data, numb_of_elements, fields_to_lines, pos);
  setData(evt.data, numb_of_elements, fields_to_lines, pos);
  pos += numb_of_elements;

  //console.log("lines"); console.log(lines);
}

function gotUpdate(evt){
  data = new DataView(evt.data);
  setid = data.getInt16(0, true);
  timestamp = data.getFloat64(2, true);

  //console.log(setid);
  //console.log(id_map);
  var fields_to_lines = id_map.get(setid)[0];
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
