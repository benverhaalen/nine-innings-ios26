// Bookkeeping transcribed from the supplied APK WrapperRenderer SetTimer/UnsetTimer/process.
#[derive(Clone,Copy,Default,Debug)]
pub struct Timer {pub id:i32,pub fire:i64,pub interval:i64,pub loss:i64}
pub struct Timers {pub slots:[Timer;32],pub max:Option<usize>}
impl Timers {
 pub fn new()->Self{Self{slots:[Timer::default();32],max:None}}
 pub fn set(&mut self,id:i32,fire:i64,interval:i64){
  let end=self.max.map_or(0,|v|v+1);
  let i=(0..end).find(|&i|self.slots[i].id==id).or_else(||(0..32).find(|&i|self.slots[i].id==0)).expect("original timer table exhausted");
  self.slots[i]=Timer{id,fire,interval,loss:0};self.max=Some(self.max.map_or(i,|v|v.max(i)));
 }
 pub fn unset(&mut self,id:i32){for i in 0..self.max.map_or(0,|v|v+1){if self.slots[i].id==id {self.slots[i]=Timer::default();}}self.collapse();}
 fn collapse(&mut self){self.max=(0..32).rev().find(|&i|self.slots[i].id!=0);}
 pub fn begin(&mut self,i:usize,now:i64)->Option<(i32,i64)>{let t=&mut self.slots[i];let late=now-t.fire+t.loss;if t.id==0||late<0{return None}t.fire=0;Some((t.id,late))}
 pub fn finish(&mut self,i:usize,late:i64){let t=&mut self.slots[i];if t.fire==0{*t=Timer::default();self.collapse();}else if t.interval!=0{t.loss=late%t.interval;}}
}
#[cfg(test)] mod tests{use super::*;
 #[test]fn one_shot_and_rearmed(){let mut t=Timers::new();t.set(7,100,16);assert_eq!(t.begin(0,99),None);assert_eq!(t.begin(0,107),Some((7,7)));t.set(7,116,16);t.finish(0,7);assert_eq!(t.slots[0].loss,7);assert_eq!(t.begin(0,109),Some((7,0)));t.finish(0,0);assert_eq!(t.max,None);}
 #[test]fn holes_reuse_order(){let mut t=Timers::new();t.set(1,100,0);t.set(2,100,0);t.unset(1);t.set(3,100,0);assert_eq!(t.slots[0].id,3);assert_eq!(t.max,Some(1));t.unset(2);assert_eq!(t.max,Some(0));}
}
