// attach to imagent

const HasRecentlyMessaged = ObjC.classes.MessageDeliveryController['- _hasRecentlyMessaged:'].implementation;
Interceptor.attach(HasRecentlyMessaged, {
    onEnter(args) {
    },
    onLeave(retval) {
        //console.log(new ObjC.Object(retval).class);
        console.log(`MessageDeliveryController hasRecentlyMessaged ${retval} -> 0x1`);
        retval.replace(ptr(0x1));
      }    
});

const HasRecentlyMessaged2 = ObjC.classes.IMDRecentsController['- hasRecentlyMessaged:'].implementation;
Interceptor.attach(HasRecentlyMessaged2, {
    onEnter(args) {
    },
    onLeave(retval) {
        //console.log(new ObjC.Object(retval).class);
        console.log(`IMDRecentsController hasRecentlyMessaged ${retval} -> 0x1`);
        retval.replace(ptr(0x1));
      }    
});
console.log('HasRecentlyMessaged will be true for all chats!')


// testitest
/*
const SendMessageImpl = ObjC.classes.MessageDeliveryController['- sendMessage:context:groupContext:toGroup:toParticipants:originallyToParticipants:fromID:fromAccount:chatIdentifier:originalPayload:replyToMessageGUID:fakeSavedReceiptBlock:completionBlock:'].implementation;
console.log("print instructions for SendMessageImpl")
let temp_ptr = SendMessageImpl
let size = 0
for (let i = 0; i < 100; i++) {
    let inst = Instruction.parse(temp_ptr)
    console.log(inst.address + ": " + inst.toString())
    temp_ptr = inst.next
}
*/