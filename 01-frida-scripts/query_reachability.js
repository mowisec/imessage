// attach to Messages



//IMServiceReachabilityContext instance: <IMServiceReachabilityContext 0x300d83280 [chatIdentifier: (null) style:  senderLastAddressedHandle: (null) SIMID: (null) lastUsedService: (null) serviceOfLastMessage: (null) wasDowngraded: NO hasHistory: NO shouldForceRefresh: NO forceMMS: NO isForPendingConversation: NO]>

// Some generic classes that we'll need
const {NSString, NSMutableArray, IMServiceReachabilityController} = ObjC.classes;
const nil = ObjC.Object(ptr("0x0"));
var controller = IMServiceReachabilityController.sharedController(); // get the instance of the registry rather than creating a new one



// "mailto:" or "tel:+"
function queryReachability(contact) {

  var contacts = NSMutableArray.array();
  contacts.addObject_(NSString.stringWithString_(contact));



  var context = ObjC.classes.IMServiceReachabilityContext.alloc().init();
  // If created as a new chat Apple sets isForPendingConversation to true; but it is not necessary
  //context.setValue_forKey_(ObjC.classes.NSNumber.numberWithBool_(true), "isForPendingConversation");
  //console.log("IMServiceReachabilityContext instance: " + context);


  // required as a callback - There is for sure a better way to do this
  var completionBlock = new ObjC.Block({
      retType: 'void',
      argTypes: ['object', 'bool'],
      implementation: function(service, success) {
        console.log("Completion block called.");
        console.log("Service: " + service);
        console.log("Success: " + success);
      }
    });

    console.log("Completion block: " + completionBlock);

    controller.calculateServiceForSendingToHandles_withContext_completionBlock_(
      contacts,
      context,
      completionBlock
    );
  }


  console.log("To query reachability: queryReachability(\"mailto:...\" ) or \"tel:+...\"");