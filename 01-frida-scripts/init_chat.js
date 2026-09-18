ObjC.schedule(ObjC.mainQueue, function () {

    const AC      = ObjC.classes.IMAccountController;
    const account = AC.sharedInstance().activeIMessageAccount();
  
    const IMHandle = ObjC.classes.IMHandle;
    const handle   = IMHandle.alloc()
          .initWithAccount_ID_alreadyCanonical_(account,
                                                "",
                                                false);
  
    console.log("✅ handle =", handle);
  
    // Build the NSArray<IMHandle *> expected by the selector
    const NSMutableArray = ObjC.classes.NSMutableArray;
    const handlesArray   = NSMutableArray.array();
    handlesArray.addObject_(handle);
  
    const IMChatRegistry = ObjC.classes.IMChatRegistry;
    const chatRegistry   = IMChatRegistry.sharedRegistry();
  
    // Frida‑style selector name (colon → underscore, trailing underscore)
    const FN = "_ck_chatForHandles_displayName_lastAddressedHandle_" +
               "lastAddressedSIMID_joinedChatsOnly_findMatchingNamedGroups_" +
               "createIfNecessary_";
  
    const chat = chatRegistry['- _ck_chatForHandles:displayName:lastAddressedHandle:lastAddressedSIMID:joinedChatsOnly:findMatchingNamedGroups:createIfNecessary:'](
        handlesArray,    // NSArray<IMHandle *> handles
        ptr('0x0'),      // displayName (nil)
        ptr('0x0'),      // lastAddressedHandle
        ptr('0x0'),      // lastAddressedSIMID
        0,               // joinedChatsOnly  (BOOL)
        0,               // findMatchingNamedGroups
        1);              // createIfNecessary
  
    console.log("✅ chat =", chat);
  });