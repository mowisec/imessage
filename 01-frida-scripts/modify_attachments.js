// attach to IMTransferAgent
const {NSData, NSString, NSCFInputStream} = ObjC.classes;

const IMTransferAgentController = ObjC.classes.IMTransferAgentController['- sendFilePath:encrypt:topic:transferID:sourceAppID:userInfo:progressBlock:completionBlock:'].implementation;
Interceptor.attach(IMTransferAgentController, {
  onEnter(args) {
    let path = new ObjC.Object(args[2]);
    let encrypt = args[3];
    let topic = new ObjC.Object(args[4]);
    let transferID = new ObjC.Object(args[5]);
    let sourceAppID = new ObjC.Object(args[6]);
    let userInfo = new ObjC.Object(args[7]);
    let progressBlock = new ObjC.Object(args[8]);
    let completionBlock = new ObjC.Object(args[9]);
    console.log(`-[IMTransferAgentController sendFilePath:${path} encrypt:${encrypt} topic:${topic} transferID:${transferID} sourceAppID:${sourceAppID} userInfo:${userInfo} progressBlock:${progressBlock} completionBlock:${completionBlock}]`);

    console.log(path.$className, path)
       
    // replace file to be sent by different one
    let newPath = "/var/tmp/foo.txt";
    args[2] = NSString.stringWithString_(newPath);

    // disable iCloud encryption
    // messages without iCloud encryption seem to get dropped at the receiver
    args[3] = ptr(0x0);
  },
  onLeave(retval) {
  }
});


// copied from sniff_https.js to display bytes sent
const CFURLRequestSetHTTPRequestBodyStream = Module.getExportByName('CFNetwork', 'CFURLRequestSetHTTPRequestBodyStream');
Interceptor.attach(CFURLRequestSetHTTPRequestBodyStream, {
  onEnter(args){
    let request = new ObjC.Object(args[0]); // type: NSMutableURLRequest
    let stream = new ObjC.Object(args[1]); // type: __NSCFInputStream
    console.log(request);
    console.log(stream);
    
    if (stream && args[1] != 0) {
      stream.open();
      let max_chunksize = 33554432; //hope that's the max?? taken from https://gateway.icloud.com/configuration/configurations/internetservices/mobileme/content/content-1.0.plist
      if (stream.hasBytesAvailable()) {
        let buf = Memory.alloc(max_chunksize);
        let read_len = stream.read_maxLength_(buf, max_chunksize);
        console.log(`read ${read_len} bytes`); // FIXME: read_len not the actual number of bytes read, probably due to bug
        console.log(buf.readByteArray(100));
        
        // re-creating stream, working code: 
        let d = ObjC.classes.NSData.alloc();
        let data = d.initWithBytes_length_(buf, new UInt64(parseInt(read_len)));
        let n = NSCFInputStream.alloc();
        let inputstream = n.initWithData_(data);
        request.setHTTPBodyStream_(inputstream);
        } else {
          console.log("no bytes available :(")
      }
    }
  },
  onLeave(retval) {
  }
});