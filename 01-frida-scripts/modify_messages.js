// attach to imagent

// Some generic classes that we'll need
const {NSString, NSPropertyListSerialization} = ObjC.classes;
const nil = ObjC.Object(ptr("0x0"));

let newT = "asdf"; //set to 0 if you don't want to set the t field
let newX = "<html><body>BLABLABLA</body></html>"; //set to 0 if you don't want to set the x field
newX = "<html><body><FILE name=\"09c9b8afaeb5ecb5-sticker.png.png\" width=\"0\" height=\"0\" datasize=\"12069\" mime-type=\"image/png\" uti-type=\"public.png\" mmcs-owner=\"MY5733969D858549138ABDEAD466D0FB95EB5F42CC20E72881655D606018310070.21F788CE698EE1A9.C01USN00\" mmcs-url=\"https://p29-content.icloud.com/MY5733969D858549138ABDEAD466D0FB95EB5F42CC20E72881655D606018310070.21F788CE698EE1A9.C01USN00\" mmcs-signature-hex=\"813EBF44A69677AA5ECA8D3460096ED4C3D6C3B021\" file-size=\"60688\" decryption-key=\"00AB986F7523802C840E21CE94EC8F40CE579A52DAF1FB9EAE3FD6CAE54BA6A7FA\"/></body></html>"
//newX = "<html><body><FILE name=\"09c9b8afaeb5ecb5-sticker.png.png\" width=\"0\" height=\"0\" datasize=\"12069\" mime-type=\"image/png\" uti-type=\"public.png\" mmcs-owner=\"MY5733969D858549138ABDEAD466D0FB95EB5F42CC20E72881655D606018310070.21F788CE698EE1A9.C01USN00\" mmcs-url=\"https://reversing.training/MY5733969D858549138ABDEAD466D0FB95EB5F42CC20E72881655D606018310070.21F788CE698EE1A9.C01USN00\" mmcs-signature-hex=\"813EBF44A69677AA5ECA8D3460096ED4C3D6C3B021\" file-size=\"60688\" decryption-key=\"00AB986F7523802C840E21CE94EC8F40CE579A52DAF1FB9EAE3FD6CAE54BA6A7FA\"/></body></html>" //BlastDoor URL error at recipient


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
        // we can return here and should not modify anything else than the ati (or just nothing at all...)
        let ati = dict.objectForKey_("ati")
        if (ati) {
            let decompressedPlist = ati['- _decompressGZIP']();
            let json = NSPropertyListSerialization.propertyListWithData_options_format_error_(decompressedPlist, 0, nil, nil);
            console.log(`ati: ${json}`);
            return;
        }

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