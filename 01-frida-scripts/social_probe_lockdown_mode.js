// attach to imagent

// Some generic classes that we'll need
const {NSString, NSPropertyListSerialization} = ObjC.classes;
const nil = ObjC.Object(ptr("0x0"));

let newT = 0; //set to 0 if you don't want to set the t field
let newX = "<html><body><s>no</s><texteffect type=\"8\">Test</texteffect></body></html>"; //set to 0 if you don't want to set the x field


const uploadMessage = ObjC.classes.MessageDeliveryController['- idsOptionsWithMessageItem:toID:fromID:sendGUIDData:alternateCallbackID:isBusinessMessage:chatIdentifier:requiredRegProperties:interestingRegProperties:requiresLackOfRegProperties:deliveryContext:isGroupChat:canInlineAttachments:msgPayloadUploadDictionary:messageDictionary:'].implementation;
Interceptor.attach(uploadMessage, {
    onEnter(args) {
        let dict = new ObjC.Object(args[15]);
        let msgDict = args[16];  // seems to be used as message type
        console.log(`MessageDeliveryController\nmsgPayloadUploadDictionary:${dict}\nmessageDictionary:${msgDict}`);

        if (dict == null) {
            return;
        }

        let t = dict.objectForKey_("t")
        if (t == null) {
            console.log("Empty Message (Typing Indicator)");
            return;
        } else {
            console.log(`t: ${t}`);
        }

        // memojis are gzip+bplist encoded before upload
        // IMTransferagent first has to upload them and reformat the x field.
        // we can still change the t and x fields
        let ati = dict.objectForKey_("ati")

        //_NSCFString
        if (newT) {
            console.log(`Replacing the t field with ${newT}`);
            dict['- setObject:forKey:'](NSString.stringWithString_(newT), NSString.stringWithString_("t"));
        }

        //_NSCFString
        
        if (newX) {
            console.log(`Replacing the x field with ${newX}`);
            dict['- setObject:forKey:'](NSString.stringWithString_(newX), NSString.stringWithString_("x"));
        }
        
        console.log(dict);

        console.log(`-------------------${t}---------------------\n\n`)
    }
});


console.log("waiting to replace outgoing messages...")