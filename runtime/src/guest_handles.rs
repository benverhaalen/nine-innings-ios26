pub struct Handles{next:u32,end:u32,free:Vec<u32>}
impl Handles{pub fn new(start:u32,end:u32)->Self{Self{next:start,end,free:Vec::new()}}pub fn allocate(&mut self)->u32{if let Some(h)=self.free.pop(){return h}assert!(self.next<self.end,"JNI handle arena exhausted");let h=self.next;self.next+=4;h}pub fn release(&mut self,h:u32){assert!(h<self.next&&h%4==0);self.free.push(h);}}
#[cfg(test)]mod tests{use super::*;#[test]fn handles_do_not_collide_after_collection(){let mut h=Handles::new(0x2320000,0x2330000);let live=h.allocate();for _ in 0..100000{let temp=h.allocate();assert_ne!(temp,live);h.release(temp);}assert_eq!(h.next,0x2320008);}}

pub fn roots(local:&std::collections::HashMap<(u32,u32),u32>,global:&std::collections::HashMap<u32,u32>,pinned:&std::collections::HashMap<u32,u32>,children:impl Iterator<Item=u32>)->std::collections::HashSet<u32>{
 local.iter().filter(|(_,n)|**n>0).map(|((_,h),_)|*h).chain(global.iter().filter(|(_,n)|**n>0).map(|(h,_)|*h)).chain(pinned.iter().filter(|(_,n)|**n>0).map(|(h,_)|*h)).chain(children).collect()
}
#[cfg(test)]mod lifetime_tests{use super::*;use std::collections::HashMap;
 #[test]fn preserve_other_threads_globals_pins_and_container_children(){let local=HashMap::from([((0,1),0),((1,1),1),((0,2),0)]);let global=HashMap::from([(3,1),(4,0)]);let pins=HashMap::from([(5,1),(6,0)]);let r=roots(&local,&global,&pins,[7,8].into_iter());assert_eq!(r,std::collections::HashSet::from([1,3,5,7,8]));}
 #[test]fn local_to_global_transfer_survives_then_becomes_collectible(){let mut local=HashMap::from([((0,9),1)]);let mut global=HashMap::from([(9,1)]);local.insert((0,9),0);assert!(roots(&local,&global,&HashMap::new(),[].into_iter()).contains(&9));global.insert(9,0);assert!(!roots(&local,&global,&HashMap::new(),[].into_iter()).contains(&9));}
}
