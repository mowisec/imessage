// attach to imagent

// Some generic classes that we'll need
const {NSString, NSData, NSDictionary, NSArray, IMDaemonChatSendMessageRequestHandler, IMDMessageStore, __NSTaggedDate, __NSCFString, NSTaggedPointerString, __NSSingleEntryDictionaryI, NSConcreteMutableAttributedString, IMMessageItem, NSPropertyListSerialization} = ObjC.classes;
const nil = ObjC.Object(ptr("0x0"));
const macMini = NSString.stringWithString_("");
const openBubbles = NSString.stringWithString_("");


function getTimestamp() {
    return new Date().toISOString();
}

function logWithTimestamp(message) {
    console.log(`[${getTimestamp()}] ${message}`);
}

// This is the top level message handler, used in RX and TX direction.
const IDSincomingTopLevelMessage = ObjC.classes.IMDiMessageIDSDelegate['- service:account:incomingTopLevelMessage:fromID:messageContext:'].implementation;
Interceptor.attach(IDSincomingTopLevelMessage, {
    onEnter(args) {
        let topLevel = new ObjC.Object(args[4]); // Message dictionary
        logWithTimestamp(`New Incoming Message:\n\x1b[34m${topLevel}\x1b[0m`);

        // topLevel contains what we're going to decode.
        // IDSIncomingMessagePushPayload - this is still encrypted
        // but identity services decrypted it us into IDSIncomingMessageDecryptedData
        let IDSIncomingMessageDecryptedData = topLevel.valueForKey_(NSString.stringWithString_("IDSIncomingMessageDecryptedData"))
        if (IDSIncomingMessageDecryptedData) {
            logWithTimestamp("Unpacked IDSIncomingMessageDecryptedData:");
            let decompressedPlist = IDSIncomingMessageDecryptedData['- _decompressGZIP']()   // NSData can already gunzip this!
            if (! decompressedPlist) {
                console.log("IDSIncomingMessageDecryptedData (gzip decompression failed)...");
                decompressedPlist = IDSIncomingMessageDecryptedData;
            }
            let json = NSPropertyListSerialization.propertyListWithData_options_format_error_(decompressedPlist, 0, nil, nil);
            if (! json) {
                console.log("IDSIncomingMessageDecryptedData (plist to json failed):");
                console.log(hexdump(decompressedPlist.bytes(), { length: decompressedPlist.length(), ansi: true }));
                console.log();
                return;
            } else {
                console.log("IDSIncomingMessageDecryptedData:");
                console.log(`\x1b[32m${json}\x1b[0m`);
                console.log();
                return;
            }

        }
    }
});


// RX: Print message info as it goes to BlastDoor.
const BlastDoorMessage = ObjC.classes.IMTextMessagePipelineParameter['- initWithBD:idsTrustedData:'].implementation;
Interceptor.attach(BlastDoorMessage, {
    onEnter(args) {
        let bd = new ObjC.Object(args[2]);
        logWithTimestamp(`BlastDoor Message:\n\x1b[34m${bd}\x1b[0m`);
        // The .description does not contain all fields of the BD message. Print them all!
        // Problem: the BlastDoorMessage class has different implementations depending on iOS version

        /*
        console.log(`BlastDoor Message:\x1b[34m` +
            `\n\tmessageSubType: ${bd.messageSubType()}` +
            `\n\tmetadata: ${bd.metadata()}` +
            `\n\tmessageSummaryInfo: ${bd.messageSummaryInfo()}` +
            `\n\tgroupID: ${bd.groupID()}` +
            `\n\tencryptionType: ${bd.encryptionType()}` +
            `\n\treplyToGUID: ${bd.replyToGUID()}` +
            `\n\tisAutoReply: ${bd.isAutoReply()}` +
            `\n\tthreadIdentifierGUID: ${bd.threadIdentifierGUID()}` +
            `\n\texpressiveSendStyleIdentifier: ${bd.expressiveSendStyleIdentifier()}` +
            `\n\tcurrentGroupName: ${bd.currentGroupName()}` +
            `\n\thas_groupParticipantVersion: ${bd.has_groupParticipantVersion}` +
            `\n\tgroupParticipantVersion: ${bd.groupParticipantVersion()}` +
            `\n\thas_groupProtocolVersion: ${bd.has_groupProtocolVersion}` +
            `\n\tgroupProtocolVersion: ${bd.groupProtocolVersion()}` +
            `\n\thas_groupPhotoCreationTime: ${bd.has_groupPhotoCreationTime}` +
            `\n\tgroupPhotoCreationTime: ${bd.groupPhotoCreationTime()}` +
            `\n\tavailabilityVerificationRecipientChannelIDPrefix: ${bd.availabilityVerificationRecipientChannelIDPrefix()}` +
            `\n\tavailabilityVerificationRecipientEncryptionValidationToken: ${bd.availabilityVerificationRecipientEncryptionValidationToken()}` +
            `\n\tnicknameInformation: ${bd.nicknameInformation()}` +
            `\n\ttruncatedNicknameRecordKey: ${bd.truncatedNicknameRecordKey()}\x1b[0m\n`);
            */
    }
});

// RX: Easiest way to intercept the printable plain text body, helps us with debugging.

const PlainTextBody = ObjC.classes.IMTextMessagePipelineParameter['- setPlainTextBody:'].implementation;
Interceptor.attach(PlainTextBody, {
    onEnter(args) {
        let body = new ObjC.Object(args[2]);

        // Typing indicators are empty messages,
        // text messages use the class NSTaggedPointerString
        if (body.$className.localeCompare("nil") != 0) {
            logWithTimestamp(`Plaintext Body:\n\t\x1b[34m${body}\x1b[0m\n`);
            //body = NSString.stringWithString_("Trololol");  // doesn't work to replace in rx direction but ok
        } else {
            logWithTimestamp(`Typing Indicator\n`);
        }

        console.log("-----------------------"); // end of message debug print
    }
});


// TX: First an `IMMessageItem` object is initialized and then sent through this function.
const IMSendMessage = ObjC.classes.IMDaemonChatSendMessageRequestHandler['- sendMessage:toChatID:identifier:style:account:'].implementation;
Interceptor.attach(IMSendMessage, {
    onEnter(args) {

        let sendMessage = new ObjC.Object(args[2]);
        let toChatID = new ObjC.Object(args[3]);
        let identifier = new ObjC.Object(args[4]);
        let style = args[5];  // not an object, just an enum or so
        let account = new ObjC.Object(args[6]);
        logWithTimestamp(`Send Message: \x1b[34m${sendMessage} (${sendMessage.$className})\x1b[0m` +
            `\n\tTo Chat ID: \x1b[34m${toChatID} (${toChatID.$className})\x1b[0m` +
            `\n\tIdentifier: \x1b[34m${identifier} (${identifier.$className})\x1b[0m`+ 
            `\n\tStyle: \x1b[34m${style}\x1b[0m`+ 
            `\n\tAccount: \x1b[34m${account} (${account.$className})\x1b[0m\n`);

        // The IMMessageItem has a lot of methods!
        // We can likely also set the body etc. here.
        //console.log(sendMessage.$ownMethods)

        // Body class: NSConcreteMutableAttributedString, which consists of
        // an NSString + an NSDictionary of attributes. The range can be a
        // null pointer.
        let body = sendMessage.body();
        console.log(`Message Body: ${sendMessage.body()}`);

        // Exemplarily setting the body to something new, but we could set
        // a lot of other attributes here as well!
        // Comment out if you want to replace the text.

        /*
        //let newAttributes = __NSSingleEntryDictionaryI.alloc().initWithObject_forKey_(0, NSString.stringWithString_("__kIMMessagePartAttributeName"));
        let newBody = NSConcreteMutableAttributedString.alloc().initWithString_("Replaced with a new message :)");
        //newBody.setAttributes_range_(newAttributes, 0);  // FIXME: this somehow does not work
        sendMessage.setBody_(newBody);
        console.log(`New Message Body: ${sendMessage.body()}`);
        */

    }
});





// -[_IDSConnection messageIdentifier:alternateCallbackID:forAccount:willSendToDestinations:skippedDestinations:registrationPropertyToDestinations:]
// is called multiple times, only then we call into the function we hook here. So if we want to modify destinations, that probably needs
// to happen earlier - but this is fine to print them!
// Solution? -[_IDSService connection:identifier:alternateCallbackID: illSendToDestinations:skippedDestinations:registrationPropertyToDestinations:] is called afterwards (but a few times more often)
const DeliveryController = ObjC.classes.MessageDeliveryController['- service:account:identifier:alternateCallbackID:willSendToDestinations:skippedDestinations:registrationPropertyToDestinations:'].implementation;
//const IDSServiceConnection = ObjC.classes._IDSService['- connection:identifier:alternateCallbackID:willSendToDestinations:skippedDestinations:registrationPropertyToDestinations:'].implementation;
Interceptor.attach(DeliveryController, {
    onEnter(args) {

        // Here we can see that we send the message to ourselves and to the other's Apple ID
        let willSendToDestinations = new ObjC.Object(args[6]);
        // do not print empty dictionaries
        if (willSendToDestinations.$className.localeCompare("__NSDictionary0") != 0) {
            console.log(`Message arrived at DeliveryController and will be sent to the following destinations: \x1b[34m${willSendToDestinations}\x1b[0m`);
            let mutableCopy = willSendToDestinations.mutableCopy();
            if (!mutableCopy.containsObject_(openBubbles))
                mutableCopy.addObject_(openBubbles);
            if (!mutableCopy.containsObject_(macMini))
                mutableCopy.addObject_(macMini);
            //args[6] = mutableCopy;
            console.log(`Fix Destination list: \x1b[34m${ObjC.Object(args[6])}\x1b[0m`);
        }

        //args[7] = ObjC.classes.NSMutableArray.alloc().init();
        //args[8] = ObjC.classes.NSMutableDictionary.alloc().init();
        //console.log(`${new ObjC.Object(args[7]).class()}`);
        //console.log(`${new ObjC.Object(args[8]).class()}`);


        let skippedDestinations = new ObjC.Object(args[7]);
        // do not print empty dictionaries
        if (skippedDestinations.$className.localeCompare("__NSArray0") != 0) {
            console.log(`Message arrived at DeliveryController and the following destionations will be skipped: \x1b[34m${skippedDestinations}\x1b[0m`);
        }

        // This seems to be set for typing indicators
        let registrationPropertyToDestinations = new ObjC.Object(args[8]);
        // do not print empty dictionaries
        if (registrationPropertyToDestinations.$className.localeCompare("__NSDictionary0") != 0) {
            console.log(`Message arrived at DeliveryController and will be registered to the following destinations: \x1b[34m${registrationPropertyToDestinations}\x1b[0m`);
        }
    }
});

// imagent uses XPC to communicate with identityservicesd to exchange send parameters.
// Let's print them here, as they are related to the encryption!
// After everything is set and before they go over XPC, the dictionary representation is used.
// Might be interesting to change them later.
const sendParameters = ObjC.classes.IDSSendParameters['- dictionaryRepresentation'].implementation;
Interceptor.attach(sendParameters, {
    onLeave(retval) {
        console.log(`IDSSendParameters: \x1b[34m${new ObjC.Object(retval)}\x1b[0m`);
        //retval = new ObjC.Object(retval).mutableCopy();
        //retval.removeObjectForKey_("IDSSendParametersRequireLackOfRegistrationPropertiesKey");
        //console.log(`${retval.class()}`);
        //console.log(`IDSSendParameters: \x1b[34m${new ObjC.Object(retval)}\x1b[0m`);
    }
});

const uploadMessage = ObjC.classes.MessageDeliveryController['- idsOptionsWithMessageItem:toID:fromID:sendGUIDData:alternateCallbackID:isBusinessMessage:chatIdentifier:requiredRegProperties:interestingRegProperties:requiresLackOfRegProperties:deliveryContext:isGroupChat:canInlineAttachments:msgPayloadUploadDictionary:messageDictionary:'].implementation;
Interceptor.attach(uploadMessage, {
    onEnter(args) {

        /*console.log('MessageDeliveryController called from:\n' +
            Thread.backtrace(this.context, Backtracer.ACCURATE)
            .map(DebugSymbol.fromAddress).join('\n') + '\n');
*/
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

        console.log(`-------------------${t}---------------------\n\n`)
    }
});

// transcodeURL seems helpful; called when file attachment sent, not used with stickers since it has the local path of the object (stickers probably don't need that)
const messageAttachmentController = ObjC.classes.MessageAttachmentController['- _transcodeURL:reason:transfer:sizes:commonCapabilities:sendStatus:urlGroup:didTranscode:handleURL:'].implementation
Interceptor.attach(messageAttachmentController, {
    onEnter(args) {
        let _transcodeURL = new ObjC.Object(args[2]);
        let reason = args[3];
        let transfer = new ObjC.Object(args[4]);

        let commonCapabilities = new ObjC.Object(args[6]);
        let sendStatus = new ObjC.Object(args[7]);
        let urlGroup = new ObjC.Object(args[8]);
        let didTranscode = new ObjC.Object(args[9]);
        let handleURL = new ObjC.Object(args[10]);
        console.log(`\nmessageAttachmentController: transcodeURL ${_transcodeURL}, reason ${reason}, transfer ${transfer}, commonCapabilities ${commonCapabilities}, sendStatus ${sendStatus}, urlGroup ${urlGroup}, didTranscode ${didTranscode}, handleURL ${handleURL}`);
    }
});

// called when sharing from "files" app called 
// then: messageAttachmentController transcodeURL
// then: messageAttachmentController _sendURL
// or: directly initWithSenderInfo upon selecting recipient
const eagerUploadKeyForURL = ObjC.classes.MessageAttachmentController['- eagerUploadKeyForURL:sizes:transcodeDictionary:capabilities:'].implementation;
Interceptor.attach(eagerUploadKeyForURL, {
    onEnter(args){
        let eagerUploadKeyForURL = new ObjC.Object(args[2]);
        let transcodeDictionary = new ObjC.Object(args[4]);
        let capabilities = new ObjC.Object(args[5])
        console.log(`\neagerUploadKeyForURL ${eagerUploadKeyForURL}, transcodeDict ${transcodeDictionary}, capabilities ${capabilities}`);
    },
    // returns some file transfer GUID
    onLeave(retval) {
        let ret = new ObjC.Object(retval);
        console.log(`eagerUploadKeyForURL returns: ${ret}`);
    }
});

const _sendURL = ObjC.classes.MessageAttachmentController['- _sendURL:urlToRemove:topic:sessionInfo:messageGUID:transferID:fileTransferGUID:attachmentSendContexts:failIfError:sendStatus:attachmentStatus:group:'].implementation;
Interceptor.attach(_sendURL, {
    onEnter(args){
        let _sendURL = new ObjC.Object(args[2]);
        let urlToRemove = new ObjC.Object(args[3]);
        let topic = new ObjC.Object(args[4]);
        let sessionInfo = new ObjC.Object(args[5]);
        let messageGUID = new ObjC.Object(args[6]);
        let transferID = new ObjC.Object(args[7]);
        let fileTransferGUID = new ObjC.Object(args[8]);
        let attachmentSendContexts = new ObjC.Object(args[9]);
        let failIfError = args[10];
        let sendStatus = new ObjC.Object(args[11]);
        let attachmentStatus = new ObjC.Object(args[12]);
        let group = new ObjC.Object(args[13]);
        console.log(`\n_sendURL ${_sendURL}, urlToRemove ${urlToRemove}, topic ${topic}, sessionInfo ${sessionInfo}, messageGUID ${messageGUID}, transferID ${transferID}, fileTransferGUID ${fileTransferGUID}, attachmentSendContexts ${attachmentSendContexts}, failIfError ${failIfError}, sendStatus ${sendStatus}, attachmentStatus ${attachmentStatus}, group ${group}`);
    }
});


console.log('Intercepting iMessages!')