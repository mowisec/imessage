// attach to Messages
// to change message contents with attachments, use modify_messages.js after injecting

// Some generic classes that we'll need
const {NSString, IMAccountController, IMMessage, IMHandle, CKChatController, __NSTaggedDate, IMAccount, IMChatRegistry, NSConcreteMutableAttributedString, NSNumber, NSDictionary, NSMutableArray} = ObjC.classes;
const nil = ObjC.Object(ptr("0x0"));
const chatRegistry = IMChatRegistry.sharedRegistry(); // get the instance of the registry rather than creating a new one


// use this to list all your existing contacts that you have chats with
function getIMHandles() {
    return IMAccount.arrayOfAllIMHandles();
}


function getChatCreateIfNeecessary(toAccount) {
    const account = IMAccountController.sharedInstance().activeIMessageAccount();
  
    const handle   = IMHandle.alloc()
          .initWithAccount_ID_alreadyCanonical_(account,
                                                toAccount,
                                                false);
  
    console.log("✅ handle =", handle);
  
    // Build the NSArray<IMHandle *> expected by the selector
    const handlesArray   = NSMutableArray.array();
    handlesArray.addObject_(handle);
      
    const chat = chatRegistry['- _ck_chatForHandles:displayName:lastAddressedHandle:lastAddressedSIMID:joinedChatsOnly:findMatchingNamedGroups:createIfNecessary:'](
        handlesArray,    // NSArray<IMHandle *> handles
        ptr('0x0'),      // displayName (nil)
        ptr('0x0'),      // lastAddressedHandle
        ptr('0x0'),      // lastAddressedSIMID
        1,               // joinedChatsOnly  (BOOL)
        0,               // findMatchingNamedGroups
        1);              // createIfNecessary
  
    console.log("✅ chat =", chat);
    return chat;
}

// query the chat registry to get an already started chat for the target Apple ID,
//  toAccount: iMessage account as mail address or phone number
function openExistingChat(toAccount) {
    let style = 0x2d; // is 0x2d for iMessage, note that there's also support for other styles, to be explored!
    let chat = chatRegistry['- _existingChatWithIdentifier:style:service:'](NSString.stringWithString_(toAccount), style, NSString.stringWithString_("iMessage"));
    if (chat) {
        console.log(`Chat found: ${chat}`);
    } else {
        console.error(`No existing chat found for account ID ${toAccount}!!!`);
        console.log(`Existing chats: \n${getIMHandles()}`)
    }
    return chat
}

// call this function to automatically send a message
function sendMessage(toAccount = "") {
    // Note that the IMMessage has various init functions and can have a lot of attributes.
    // Testing these might be quite interesting!
    // Use `frida-trace -U Messages -m '*[IMMessage initW*]'` to observe them in action.
    // Available init functions:
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:associatedMessageGUID:associatedMessageType:associatedMessageRange:associatedMessageInfo:
    //  - _initWithSender:time:timeRead:timeDelivered:timePlayed:plainText:text:messageSubject:fileTransferGUIDs:flags:error:guid:messageID:subject:balloonBundleID:payloadData:expressiveSendStyleID:timeExpressiveSendPlayed:associatedMessageGUID:associatedMessageType:associatedMessageRange:messageSummaryInfo:threadIdentifier:dateEdited:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:threadIdentifier:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:balloonBundleID:payloadData:expressiveSendStyleID:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:associatedMessageGUID:associatedMessageType:associatedMessageRange:messageSummaryInfo:",
    //  - initWithSender:fileTransfer:",
    //  - initWithSender:time:text:fileTransferGUIDs:flags:error:guid:subject:,
    //  - initWithSender:time:text:fileTransferGUIDs:flags:error:guid:subject:threadIdentifier:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:associatedMessageGUID:associatedMessageType:associatedMessageRange:messageSummaryInfo:threadIdentifier:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:balloonBundleID:payloadData:expressiveSendStyleID:threadIdentifier:",
    // ... this is on iOS 16.1.2, maybe there's even more for some of the new message effects! If they're not just fileTransfers.
    

    // message properties
    let chat = getChatCreateIfNeecessary(toAccount)

    let sender = IMHandle.alloc().init() //TODO, but seems to work without as we have the chat selected
    let time = __NSTaggedDate.now();
    let guid = nil; //NSString.stringWithString_("39057384-B017-42AD-9F14-534A29586915");  // message guid must exist or we will crash due to missing IMDMessageRecordRef, but if we use exissting guid we will update the message instead of sending a new one -- apparently will be set if we set it to nil
    let flags = 0x100005; // flags are required to set "outgoing: YES", typing indicator uses flag 0c - see logs which flags you need!
    let text = NSConcreteMutableAttributedString.alloc().initWithString_("REPLACEME");

    //e.g., -[IMMessage initWithSender:0xa4cc64b80 time:0xae3d25052884b7bc text:0x280def980 messageSubject:0x0 fileTransferGUIDs:0x0 flags:0x100005 error:0x0 guid:0x281cd0900 subject:0x0 threadIdentifier:0x0]
    let message = IMMessage.alloc()['- initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:threadIdentifier:'](sender, time, text, nil, nil, flags, nil, guid, nil, nil);

    // message styles can be set with setExpressiveSendStyleID
    // but to set the raw message, we must use `JWEncodeDictionary` in imagent


    console.log(`Injecting Message: ${message}`);

    chatRegistry['- _chat:sendMessage:'](chat, message);

}


function sendTyping(toAccount="", typing=true) {
    // Note that the IMMessage has various init functions and can have a lot of attributes.
    // Testing these might be quite interesting!
    // Use `frida-trace -U Messages -m '*[IMMessage initW*]'` to observe them in action.
    // Available init functions:
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:associatedMessageGUID:associatedMessageType:associatedMessageRange:associatedMessageInfo:
    //  - _initWithSender:time:timeRead:timeDelivered:timePlayed:plainText:text:messageSubject:fileTransferGUIDs:flags:error:guid:messageID:subject:balloonBundleID:payloadData:expressiveSendStyleID:timeExpressiveSendPlayed:associatedMessageGUID:associatedMessageType:associatedMessageRange:messageSummaryInfo:threadIdentifier:dateEdited:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:threadIdentifier:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:balloonBundleID:payloadData:expressiveSendStyleID:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:associatedMessageGUID:associatedMessageType:associatedMessageRange:messageSummaryInfo:",
    //  - initWithSender:fileTransfer:",
    //  - initWithSender:time:text:fileTransferGUIDs:flags:error:guid:subject:,
    //  - initWithSender:time:text:fileTransferGUIDs:flags:error:guid:subject:threadIdentifier:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:associatedMessageGUID:associatedMessageType:associatedMessageRange:messageSummaryInfo:threadIdentifier:",
    //  - initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:balloonBundleID:payloadData:expressiveSendStyleID:threadIdentifier:",
    // ... this is on iOS 16.1.2, maybe there's even more for some of the new message effects! If they're not just fileTransfers.
    

    // message properties
    let chat = getChatCreateIfNeecessary(toAccount)
    let flags = 0x0c;
    let sender = IMHandle.alloc().init(); //TODO, but seems to work without as we have the chat selected
    let time = nil;
    let guid = nil; //NSString.stringWithString_("39057384-B017-42AD-9F14-534A29586915");  // message guid must exist or we will crash due to missing IMDMessageRecordRef, but if we use exissting guid we will update the message instead of sending a new one -- apparently will be set if we set it to nil
    if (!typing) {
        flags = 0x0d; // stop typing
    }
    let text = nil;

    //e.g., -[IMMessage initWithSender:0x0 time:0x0 text:0x0 messageSubject:0x0 fileTransferGUIDs:0x0 flags:0xc error:0x0 guid:0x6000013245c0 subject:0x0 balloonBundleID:0x0 payloadData:0x0 expressiveSendStyleID:0x0 threadIdentifier:0x0] //typing
    //e.g., -[IMMessage initWithSender:0x7fe568b115c0 time:0x0 text:0x0 messageSubject:0x0 fileTransferGUIDs:0x0 flags:0xc error:0x0 guid:0x6000013245c0 subject:0x0 threadIdentifier:0x0] //typing 2nd
    //e.g., -[IMMessage initWithSender:0xa4cc64b80 time:0xae3d25052884b7bc text:0x280def980 messageSubject:0x0 fileTransferGUIDs:0x0 flags:0x100005 error:0x0 guid:0x281cd0900 subject:0x0 threadIdentifier:0x0] //normal message
    let message = IMMessage.alloc()['- initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:threadIdentifier:'](sender, time, text, nil, nil, flags, nil, guid, nil, nil);

    // message styles can be set with setExpressiveSendStyleID
    // but to set the raw message, we must use `JWEncodeDictionary` in imagent


    console.log(`Injecting Message: ${message}`);

    chatRegistry['- _chat:sendMessage:'](chat, message);

}


function sendMessageWithAttachment(toAccount = "") {
    let chat = openExistingChat(toAccount)

    // to send a custom attachment, use intercept_imessage.js to get the "transfer guid" printed by IMSendMessage
    var transferGUID = NSString.stringWithString_("F0A3D6EF-0F21-4357-AE9F-D9E6C6193ED0"); // 100.3MB file "AFD699FC-2A2F-42BD-BD5D-193368E3BB82" // apple website F0A3D6EF-0F21-4357-AE9F-D9E6C6193ED0 // empty pdf: 412DFDE1-B42C-43AC-9E71-D801048DBC3B // cow animoji: "04E7086C-FA21-405E-9119-69F9AA51A72F"
  
    // create placeholder text with attributes
    let attrObjs = NSMutableArray.alloc().initWithCapacity_(2);
    let attrKeys = NSMutableArray.alloc().initWithCapacity_(2);
    attrObjs.addObject_(transferGUID);
    attrObjs.addObject_(NSNumber.numberWithInt_(0));
    attrKeys.addObject_(NSString.stringWithString_("__kIMFileTransferGUIDAttributeName"));
    attrKeys.addObject_(NSString.stringWithString_("__kIMMessagePartAttributeName"));
    let attrDict = NSDictionary.alloc().initWithObjects_forKeys_(attrObjs, attrKeys);

    let text = NSConcreteMutableAttributedString.alloc().initWithString_attributes_(NSString.stringWithString_("\uFFFC"), attrDict); 

    // fileTransferGUID array
    let fileTransferGUIDs = NSMutableArray.alloc().initWithCapacity_(1);
    fileTransferGUIDs.addObject_(transferGUID);

    var sender = nil;
    var time = __NSTaggedDate.now(); 
    var subject = nil;
    var flags = 0x5;
    var error = nil; 
    var guid = nil; 
    var subject = nil;
    var balloonID = nil;
    var payload = nil;
    var expressiveSendStyleID = nil;
    var threadID = nil;
    var scheduleType = 0;
    var scheduleState = 0;
    var summary = nil;

    var message = IMMessage.alloc()['- initWithSender:time:text:messageSubject:fileTransferGUIDs:flags:error:guid:subject:balloonBundleID:payloadData:expressiveSendStyleID:threadIdentifier:scheduleType:scheduleState:messageSummaryInfo:'](
      sender, time, text, subject, fileTransferGUIDs, flags, error, guid, subject, balloonID, payload, expressiveSendStyleID, threadID, scheduleType, scheduleState, summary);

  console.log(`sending message ${message}`);

  chatRegistry['- _chat:sendMessage:'](chat, message);
}

console.log('To inject messages, use the `sendMessage()`, `sendTyping()` or `sendMessageWithAttachment()` function!')

rpc.exports = {
    sendMessage: sendMessage,
    sendTyping: sendTyping,
    sendMessageWithAttachment: sendMessageWithAttachment,
};