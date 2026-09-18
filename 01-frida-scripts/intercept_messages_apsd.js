// this script intercepts messages at the encrypted level within that APS daemon receives them
// attach to apsd

const APSMessageD = ObjC.classes.APSMessage['- initWithDictionary:'].implementation;
Interceptor.attach(APSMessageD, {
    onEnter(args) {
        console.log(`APSMessage\nDictionary:${new ObjC.Object(args[2])}\n-----------------`);
    }
});

const APSMessageX = ObjC.classes.APSMessage['- initWithDictionary:xpcMessage:'].implementation;
Interceptor.attach(APSMessageX, {
    onEnter(args) {
        console.log(`APSMessage\nDictionary:${new ObjC.Object(args[2])}\n-----------------`);
    }
});

const APSMessageT = ObjC.classes.APSMessage['- initWithTopic:userInfo:'].implementation;
Interceptor.attach(APSMessageT, {
    onEnter(args) {
        console.log(`APSMessage\nTopic: ${new ObjC.Object(args[2])}\tUserInfo: ${new ObjC.Object(args[3])}\n-----------------`);
    }
});