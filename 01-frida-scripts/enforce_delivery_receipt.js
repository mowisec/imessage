// attach to imagent

const SetDeliveryReceipts = ObjC.classes.IDSSendParameters['- setWantsDeliveryStatus:'].implementation;
Interceptor.attach(SetDeliveryReceipts, {
    onEnter(args) {
        if (args[2] == 0) {
            console.log(`Override setWantsDeliveryStatus: ${args[2]} -> 0x1!`)
            args[2] = ptr(1);
        }
    }
});
console.log('Delivery Receipts will be enforced for all messages!')
