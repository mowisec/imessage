const {NSString, NSData, NSDictionary, NSArray, IMDaemonChatSendMessageRequestHandler, IMDMessageStore, __NSTaggedDate, __NSCFString, NSTaggedPointerString, __NSSingleEntryDictionaryI, NSConcreteMutableAttributedString, IMMessageItem, NSPropertyListSerialization} = ObjC.classes;
const nil = ObjC.Object(ptr("0x0"));




function extractHexFromNSData(obj) {
    if (!obj || !obj.isKindOfClass_(ObjC.classes.NSData)) return null;
    const len = obj.length(), ptr = obj.bytes();
    return (ptr.isNull() || len === 0) ? null
        : Array.from(new Uint8Array(ptr.readByteArray(len)))
               .map(b => b.toString(16).padStart(2, '0'))
               .join('');
}


// from intercept_imessages.js
// get timestamp in sending direction
const sendParameters = ObjC.classes.IDSSendParameters['- dictionaryRepresentation'].implementation;
Interceptor.attach(sendParameters, {
    onLeave(retval) {
        //console.log(`IDSSendParameters: \x1b[34m${new ObjC.Object(retval)}\x1b[0m`);
        const sendObj = new ObjC.Object(retval);
        //var uuid = sendObj.valueForKey_(NSString.stringWithString_("IDSSendParametersMessageUUIDKey"));
        const uuidObj = sendObj.objectForKey_("IDSSendParametersMessageUUIDKey");
        const uuidHex = extractHexFromNSData(uuidObj);
        const keyObj = sendObj.objectForKey_("IDSSendParametersFireAndForgetKey");
        const typingIndicator = keyObj ? keyObj.toString() : 0;

        //send({direction: "outgoing", object: sendObj.toString()});
        send({direction: "outgoing", object: {uuid: uuidHex, typingIndicator: typingIndicator}});
    }
});



// from intercept_imessages.js
// get timestamp in receiving direction
const IDSincomingTopLevelMessage = ObjC.classes.IMDiMessageIDSDelegate['- service:account:incomingTopLevelMessage:fromID:messageContext:'].implementation;
Interceptor.attach(IDSincomingTopLevelMessage, {
    onEnter(args) {
        let topLevel = new ObjC.Object(args[4]); // Message dictionary
        const recvObj = topLevel.objectForKey_("IDSIncomingMessagePushPayload");
        const uuidObj = recvObj.objectForKey_("U");
        const uuidHex = extractHexFromNSData(uuidObj);
        const command = recvObj.objectForKey_("c").toString();
        const epoch = recvObj.objectForKey_("e").toString();
        const deviceTokenObj = recvObj.objectForKey_("t");
        const deviceTokenHex = extractHexFromNSData(deviceTokenObj);
        //console.log(`New Incoming Message:\n\x1b[34m${topLevel}\x1b[0m`);
        send({direction: "incoming", object: {uuid: uuidHex, command: command, epoch: epoch, deviceToken: deviceTokenHex}});
        return;

    }
});