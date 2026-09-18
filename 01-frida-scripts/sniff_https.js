// attach to IMTransferAgent
const {NSData, NSString, NSCFInputStream} = ObjC.classes;
const NSURLSessionTaskData = ObjC.classes.NSURLSession['- dataTaskWithRequest:completionHandler:'].implementation
var pendingBlocks = new Set();

Interceptor.attach(NSURLSessionTaskData, {
    onEnter(args) {
        
        // print request
        let request = new ObjC.Object(args[2]);
        console.log(request);
        console.log(request.allHTTPHeaderFields())

        
        // replace the completion handler
        const block = new ObjC.Block(args[3]);
        pendingBlocks.add(block); // Keep it alive
        const appCallback = block.implementation;
        block.implementation = (data, response, error) => {
          console.log(response);
          console.log(data)
          let data_bytes = Memory.readByteArray(data.bytes(), data.length());
          console.log("Content:")
          console.log(hexdump(data_bytes));

          const result = appCallback(data, response, error);
          pendingBlocks.delete(block);

          console.log("---^NSHTTPURLResponse (Data) -----------------------------------------")

          return result;
        };
    },
    onLeave(retval) {
    }
});

const NSURLSessionTaskRequest = ObjC.classes.NSURLSession['- uploadTaskWithRequest:fromData:'].implementation
Interceptor.attach(NSURLSessionTaskRequest, {
    onEnter(args) {
        
        // print request
        let request = new ObjC.Object(args[2]);
        console.log(request);
        console.log(request.allHTTPHeaderFields())

        
        // print data (NSData)
        let data = new ObjC.Object(args[3]);
        console.log(hexdump(data.bytes(), { length: data.length(), ansi: true }))

        console.log("---^NSURLSession uploadTask ----------------------------------------------")

    },
    onLeave(retval) {
    }
});

const NSURLSessionTaskRequestHandler = ObjC.classes.NSURLSession['- uploadTaskWithRequest:fromData:completionHandler:'].implementation
Interceptor.attach(NSURLSessionTaskRequestHandler, {
    onEnter(args) {
        
        // print request
        let request = new ObjC.Object(args[2]);
        console.log(request);
        console.log(request.allHTTPHeaderFields())

        
        // print data (NSData)
        let data = new ObjC.Object(args[3]);
        console.log(hexdump(data.bytes(), { length: data.length(), ansi: true }))

        // replace the completion handler
        const block = new ObjC.Block(args[4]);
        pendingBlocks.add(block); // Keep it alive
        const appCallback = block.implementation;
        block.implementation = (data, response, error) => {
          console.log(response);
          console.log(data)
          console.log("Content:")
          console.log(hexdump(data.bytes(), { length: data.length(), ansi: true }));

          const result = appCallback(data, response, error);
          pendingBlocks.delete(block);

          console.log("---^NSURLSession uploadTask completed -----------------------------------------")

          return result;
        };
    },
    onLeave(retval) {
    }
});


const NSURLSessionTaskRequestURL = ObjC.classes.NSURLSession['- uploadTaskWithRequest:fromFile:'].implementation
Interceptor.attach(NSURLSessionTaskRequestURL, {
    onEnter(args) {
        
        // print request
        let request = new ObjC.Object(args[2]);
        console.log(request);
        console.log(request.allHTTPHeaderFields())

        
        // print data (NSData)
        let url = new ObjC.Object(args[3]);
        console.log(url)

        console.log("---^NSURLSession uploadTask ----------------------------------------------")

    },
    onLeave(retval) {
    }
});

const NSURLSessionTaskRequestURLHandler = ObjC.classes.NSURLSession['- uploadTaskWithRequest:fromFile:completionHandler:'].implementation
Interceptor.attach(NSURLSessionTaskRequestURLHandler, {
    onEnter(args) {
        
        // print request
        let request = new ObjC.Object(args[2]);
        console.log(request);
        console.log(request.allHTTPHeaderFields())

        
        // print data (NSData)
        let url = new ObjC.Object(args[3]);
        console.log(url)

        // replace the completion handler
        const block = new ObjC.Block(args[4]);
        pendingBlocks.add(block); // Keep it alive
        const appCallback = block.implementation;
        block.implementation = (data, response, error) => {
          console.log(response);
          console.log(data)
          console.log("Content:")
          console.log(hexdump(data.bytes(), { length: data.length(), ansi: true }));

          const result = appCallback(data, response, error);
          pendingBlocks.delete(block);

          console.log("---^NSURLSession uploadTask completed -----------------------------------------")

          return result;
        };
    },
    onLeave(retval) {
    }
});


const NSCFLocalSessionTaskInit = ObjC.classes.__NSCFLocalSessionTask['- initWithOriginalRequest:ident:taskGroup:'].implementation
Interceptor.attach(NSCFLocalSessionTaskInit, {
  onEnter(args) {
      
        // print request
        let request = new ObjC.Object(args[2]);
        console.log(request.allHTTPHeaderFields());
        console.log(request);
        // body: has .body, .HTTPBody, .HTTPBodyStream - depends on content type!
        let body = request.body();
        let HTTPBoddy = request.HTTPBody();
        let HTTPBodyStream = request.HTTPBodyStream();
        if (body && body.length() != 0) // type: NSData
        {
          console.log('body:');
          console.log(body);
        } else if (HTTPBoddy) {
          console.log('HTTPBody:');
          console.log(HTTPBody);
        } else if (HTTPBodyStream) { // type: NSCFInputStream
          console.log('HTTPBodyStream:');
          console.log(HTTPBodyStream);       
        }

      console.log("---^NSCFURLLocalSessionConnection Original Request-----------------------------------------------")

  },
  onLeave(retval) {
  }
});


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
        
        // re-creating stream
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


const NSCFLocalSessionTaskResponse = ObjC.classes.__NSCFLocalSessionTask['- connection:didReceiveResponse:completion:'].implementation;
Interceptor.attach(NSCFLocalSessionTaskResponse, {
  onEnter(args) {
      
      // print connection
      let connection = new ObjC.Object(args[2]);
      console.log(connection);
      
      // replace the completion handler
      const block = new ObjC.Block(args[4]);
      pendingBlocks.add(block); // Keep it alive
      const appCallback = block.implementation;
      block.implementation = (data, response, error) => {
        console.log(data)
        console.log(response)
        console.log(error)

        const result = appCallback(data, response, error);
        pendingBlocks.delete(block);

        console.log("---^NSCFURLLocalSessionConnection Response-----------------------------------------------")

        return result;
      };
  },
  onLeave(retval) {
  }
});


const NSCFLocalSessionTaskData = ObjC.classes.__NSCFLocalSessionTask['- connection:didReceiveData:completion:'].implementation
Interceptor.attach(NSCFLocalSessionTaskData, {
  onEnter(args) {
      
      // print connection
      let connection = new ObjC.Object(args[2]);
      console.log(connection);

      // print data
      let data = new ObjC.Object(args[3]);
      console.log(data);
      console.log("Received Data:")
      console.log(hexdump(data.bytes(), { length: data.length(), ansi: true }));


      // replace the completion handler
      const block = new ObjC.Block(args[4]);
      pendingBlocks.add(block); // Keep it alive
      const appCallback = block.implementation;
      block.implementation = (data, response, error) => {
        console.log(data)
        console.log(response)
        console.log(error)

        const result = appCallback(data, response, error);
        pendingBlocks.delete(block);

        console.log("---^NSCFURLLocalSessionConnection Data-----------------------------------------------")

        return result;
      };
  },
  onLeave(retval) {
  }
});

const NSCFLocalSessionTaskAuth = ObjC.classes.__NSCFLocalSessionTask['- connection:challenged:authCallback:'].implementation
Interceptor.attach(NSCFLocalSessionTaskAuth, {
  onEnter(args) {
      
      // print connection
      let connection = new ObjC.Object(args[2]);
      console.log(connection);
      
      // replace the completion handler
      const block = new ObjC.Block(args[4]);
      pendingBlocks.add(block); // Keep it alive
      const appCallback = block.implementation;
      block.implementation = (data, response, error) => {
        console.log(data)
        console.log(response)
        console.log(error)

        const result = appCallback(data, response, error);
        pendingBlocks.delete(block);

        console.log("---^NSCFURLLocalSessionConnection Auth Callback-----------------------------------------------")

        return result;
      };
  },
  onLeave(retval) {
  }
});


console.log("sniffing traffic :)")
